use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::Duration;

use super::{DIAGNOSTIC_COUNTER_ORDERING, duration_to_nanos};

#[derive(Debug, Default)]
pub(super) struct RuntimeGlobalCounters {
    active_isolates: AtomicUsize,
    queued_invocations: AtomicUsize,
    worker_dispatched_invocations: AtomicU64,
    worker_affinity_routed_invocations: AtomicU64,
    worker_least_loaded_routed_invocations: AtomicU64,
    worker_affinity_cache_entries: AtomicUsize,
    worker_affinity_cache_evictions: AtomicU64,
    retained_runtime_pool_entries: AtomicUsize,
    retained_runtime_pool_evictions: AtomicU64,
    retained_runtime_pool_retirements: AtomicU64,
    retained_runtime_main_realm_resets: AtomicU64,
    retained_runtime_main_realm_reset_nanos_total: AtomicU64,
    retained_runtime_bootstrap_replays: AtomicU64,
    retained_runtime_bootstrap_replay_nanos_total: AtomicU64,
    bundle_loads: AtomicU64,
    bundle_load_nanos_total: AtomicU64,
    bundle_module_loads: AtomicU64,
    bundle_module_load_nanos_total: AtomicU64,
    bundle_evaluations: AtomicU64,
    bundle_evaluation_nanos_total: AtomicU64,
    isolate_pool_hits: AtomicU64,
    isolate_pool_misses: AtomicU64,
    isolate_pool_replacements: AtomicU64,
    started_invocations: AtomicU64,
    completed_invocations: AtomicU64,
    queue_wait_nanos_total: AtomicU64,
    execution_nanos_total: AtomicU64,
    timed_out_invocations: AtomicU64,
    canceled_invocations: AtomicU64,
    rejected_invocations: AtomicU64,
    queued_canceled_invocations: AtomicU64,
    in_flight_canceled_invocations: AtomicU64,
    disconnect_canceled_invocations: AtomicU64,
    explicit_canceled_invocations: AtomicU64,
    canceled_host_ops: AtomicU64,
    precanceled_host_ops: AtomicU64,
    in_flight_canceled_host_ops: AtomicU64,
    nested_local_dispatches: AtomicU64,
    fallback_cross_isolate_dispatches: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct RuntimeGlobalCountersSnapshot {
    pub active_isolates: usize,
    pub queued_invocations: usize,
    pub worker_dispatched_invocations: u64,
    pub worker_affinity_routed_invocations: u64,
    pub worker_least_loaded_routed_invocations: u64,
    pub worker_affinity_cache_entries: usize,
    pub worker_affinity_cache_evictions: u64,
    pub retained_runtime_pool_entries: usize,
    pub retained_runtime_pool_evictions: u64,
    pub retained_runtime_pool_retirements: u64,
    pub retained_runtime_main_realm_resets: u64,
    pub retained_runtime_main_realm_reset_nanos_total: u64,
    pub retained_runtime_bootstrap_replays: u64,
    pub retained_runtime_bootstrap_replay_nanos_total: u64,
    pub bundle_loads: u64,
    pub bundle_load_nanos_total: u64,
    pub bundle_module_loads: u64,
    pub bundle_module_load_nanos_total: u64,
    pub bundle_evaluations: u64,
    pub bundle_evaluation_nanos_total: u64,
    pub isolate_pool_hits: u64,
    pub isolate_pool_misses: u64,
    pub isolate_pool_replacements: u64,
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
    pub fallback_cross_isolate_dispatches: u64,
}

impl RuntimeGlobalCounters {
    pub(super) fn increment_queued_invocations(&self) {
        self.queued_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn decrement_queued_invocations(&self) {
        self.queued_invocations
            .fetch_sub(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn increment_active_isolates(&self) {
        self.active_isolates
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_invocation_started(&self) {
        self.started_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_worker_dispatch(&self) {
        self.worker_dispatched_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_worker_affinity_route(&self) {
        self.worker_affinity_routed_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_worker_least_loaded_route(&self) {
        self.worker_least_loaded_routed_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn update_worker_affinity_cache_entries(&self, entries: usize) {
        self.worker_affinity_cache_entries
            .store(entries, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_worker_affinity_cache_eviction(&self) {
        self.worker_affinity_cache_evictions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn increment_retained_runtime_pool_entries(&self) {
        self.retained_runtime_pool_entries
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn decrement_retained_runtime_pool_entries(&self) {
        self.retained_runtime_pool_entries
            .fetch_sub(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_retained_runtime_pool_eviction(&self) {
        self.retained_runtime_pool_evictions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_retained_runtime_pool_retirement(&self) {
        self.retained_runtime_pool_retirements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_retained_runtime_main_realm_reset(&self, duration: Duration) {
        self.retained_runtime_main_realm_resets
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.retained_runtime_main_realm_reset_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_retained_runtime_bootstrap_replay(&self, duration: Duration) {
        self.retained_runtime_bootstrap_replays
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.retained_runtime_bootstrap_replay_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_bundle_load(&self, duration: Duration) {
        self.bundle_loads.fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.bundle_load_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_bundle_module_load(&self, duration: Duration) {
        self.bundle_module_loads
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.bundle_module_load_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_bundle_evaluation(&self, duration: Duration) {
        self.bundle_evaluations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.bundle_evaluation_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_isolate_pool_hit(&self) {
        self.isolate_pool_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_isolate_pool_miss(&self) {
        self.isolate_pool_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_isolate_pool_replacement(&self) {
        self.isolate_pool_replacements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn decrement_active_isolates(&self) {
        self.active_isolates
            .fetch_sub(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_invocation_completed(&self) {
        self.completed_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_queue_wait(&self, duration: Duration) {
        self.queue_wait_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_execution(&self, duration: Duration) {
        self.execution_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_timeout(&self) {
        self.timed_out_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_canceled_invocation(&self) {
        self.canceled_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_rejected_invocation(&self) {
        self.rejected_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_queued_canceled_invocation(&self) {
        self.queued_canceled_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.record_canceled_invocation();
    }

    pub(super) fn record_in_flight_canceled_invocation(&self) {
        self.in_flight_canceled_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.record_canceled_invocation();
    }

    pub(super) fn record_disconnect_canceled_invocation(&self) {
        self.disconnect_canceled_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_explicit_canceled_invocation(&self) {
        self.explicit_canceled_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_canceled_host_op(&self) {
        self.canceled_host_ops
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_precanceled_host_op(&self) {
        self.precanceled_host_ops
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.record_canceled_host_op();
    }

    pub(super) fn record_in_flight_canceled_host_op(&self) {
        self.in_flight_canceled_host_ops
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.record_canceled_host_op();
    }

    pub(super) fn record_nested_local_dispatch(&self) {
        self.nested_local_dispatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_fallback_cross_isolate_dispatch(&self) {
        self.fallback_cross_isolate_dispatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn snapshot(&self) -> RuntimeGlobalCountersSnapshot {
        RuntimeGlobalCountersSnapshot {
            active_isolates: self.active_isolates.load(DIAGNOSTIC_COUNTER_ORDERING),
            queued_invocations: self.queued_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_dispatched_invocations: self
                .worker_dispatched_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_affinity_routed_invocations: self
                .worker_affinity_routed_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_least_loaded_routed_invocations: self
                .worker_least_loaded_routed_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_affinity_cache_entries: self
                .worker_affinity_cache_entries
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_affinity_cache_evictions: self
                .worker_affinity_cache_evictions
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_pool_entries: self
                .retained_runtime_pool_entries
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_pool_evictions: self
                .retained_runtime_pool_evictions
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_pool_retirements: self
                .retained_runtime_pool_retirements
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_main_realm_resets: self
                .retained_runtime_main_realm_resets
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_main_realm_reset_nanos_total: self
                .retained_runtime_main_realm_reset_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_bootstrap_replays: self
                .retained_runtime_bootstrap_replays
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            retained_runtime_bootstrap_replay_nanos_total: self
                .retained_runtime_bootstrap_replay_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_loads: self.bundle_loads.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_load_nanos_total: self
                .bundle_load_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_module_loads: self.bundle_module_loads.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_module_load_nanos_total: self
                .bundle_module_load_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_evaluations: self.bundle_evaluations.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_evaluation_nanos_total: self
                .bundle_evaluation_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            isolate_pool_hits: self.isolate_pool_hits.load(DIAGNOSTIC_COUNTER_ORDERING),
            isolate_pool_misses: self.isolate_pool_misses.load(DIAGNOSTIC_COUNTER_ORDERING),
            isolate_pool_replacements: self
                .isolate_pool_replacements
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            started_invocations: self.started_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            completed_invocations: self.completed_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            queue_wait_nanos_total: self
                .queue_wait_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            execution_nanos_total: self.execution_nanos_total.load(DIAGNOSTIC_COUNTER_ORDERING),
            timed_out_invocations: self.timed_out_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            canceled_invocations: self.canceled_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            rejected_invocations: self.rejected_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            queued_canceled_invocations: self
                .queued_canceled_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            in_flight_canceled_invocations: self
                .in_flight_canceled_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            disconnect_canceled_invocations: self
                .disconnect_canceled_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            explicit_canceled_invocations: self
                .explicit_canceled_invocations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            canceled_host_ops: self.canceled_host_ops.load(DIAGNOSTIC_COUNTER_ORDERING),
            precanceled_host_ops: self.precanceled_host_ops.load(DIAGNOSTIC_COUNTER_ORDERING),
            in_flight_canceled_host_ops: self
                .in_flight_canceled_host_ops
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            nested_local_dispatches: self
                .nested_local_dispatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            fallback_cross_isolate_dispatches: self
                .fallback_cross_isolate_dispatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
        }
    }
}
