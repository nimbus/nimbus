use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::Duration;

use crate::limits::{
    RuntimeAdaptiveControllerMode, RuntimeAdaptiveWarmPoolActuationKind,
    RuntimeAdaptiveWarmPoolEvaluation, RuntimeHostPressureLevel, RuntimeHostPressureSourceStatus,
    RuntimeHostResourceDecision, RuntimeMemoryPressureLevel, RuntimeMemoryPressureSourceStatus,
};

use super::{
    DIAGNOSTIC_COUNTER_ORDERING, RuntimeAdaptiveControllerMetricsSnapshot,
    RuntimeHostPressureMetricsSnapshot, duration_to_nanos,
};

#[derive(Debug, Default)]
pub(super) struct RuntimeGlobalCounters {
    active_runtime_instances: AtomicUsize,
    queued_invocations: AtomicUsize,
    worker_dispatched_invocations: AtomicU64,
    worker_affinity_routed_invocations: AtomicU64,
    worker_least_loaded_routed_invocations: AtomicU64,
    request_correlation_records: AtomicU64,
    request_correlation_nanos_total: AtomicU64,
    execution_plan_builds: AtomicU64,
    execution_plan_build_nanos_total: AtomicU64,
    admission_decisions: AtomicU64,
    admission_decision_nanos_total: AtomicU64,
    worker_router_dispatches: AtomicU64,
    worker_router_dispatch_nanos_total: AtomicU64,
    worker_affinity_cache_entries: AtomicUsize,
    worker_affinity_cache_evictions: AtomicU64,
    retained_runtime_pool_entries: AtomicUsize,
    retained_runtime_pool_evictions: AtomicU64,
    retained_runtime_pool_retirements: AtomicU64,
    bundle_loads: AtomicU64,
    bundle_load_nanos_total: AtomicU64,
    bundle_integrity_verifications: AtomicU64,
    bundle_integrity_verify_nanos_total: AtomicU64,
    bundle_module_loads: AtomicU64,
    bundle_module_load_nanos_total: AtomicU64,
    bundle_evaluations: AtomicU64,
    bundle_evaluation_nanos_total: AtomicU64,
    runtime_pool_hits: AtomicU64,
    runtime_pool_misses: AtomicU64,
    runtime_pool_replacements: AtomicU64,
    v8_startup_snapshot_runtime_constructions: AtomicU64,
    v8_unsnapshotted_runtime_constructions: AtomicU64,
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
    host_bridge_calls: AtomicU64,
    host_bridge_call_nanos_total: AtomicU64,
    nested_local_dispatches: AtomicU64,
    fallback_cross_runtime_dispatches: AtomicU64,
    warm_pool_hits: AtomicU64,
    warm_pool_misses: AtomicU64,
    warm_pool_retirements: AtomicU64,
    warm_pool_discard_unquiesced: AtomicU64,
    wasmtime_module_cache_hits: AtomicU64,
    wasmtime_module_cache_misses: AtomicU64,
    wasmtime_module_compilations: AtomicU64,
    wasmtime_module_compilation_nanos_total: AtomicU64,
    wasmtime_fuel_consumed_total: AtomicU64,
    wasmtime_fuel_exhaustions: AtomicU64,
    wasmtime_store_pool_hits: AtomicU64,
    wasmtime_store_pool_misses: AtomicU64,
    wasmtime_store_pool_authority_mismatches: AtomicU64,
    wasmtime_store_pool_evictions: AtomicU64,
    wasmtime_store_pool_retirements: AtomicU64,
    host_pressure_decisions: AtomicU64,
    host_pressure_nominal_decisions: AtomicU64,
    host_pressure_high_decisions: AtomicU64,
    host_pressure_critical_decisions: AtomicU64,
    host_pressure_cpu_source_unavailable_decisions: AtomicU64,
    host_pressure_memory_source_unavailable_decisions: AtomicU64,
    host_pressure_latest_host_level: AtomicUsize,
    host_pressure_latest_cpu_level: AtomicUsize,
    host_pressure_latest_cpu_source_status: AtomicUsize,
    host_pressure_latest_memory_level: AtomicUsize,
    host_pressure_latest_memory_source_status: AtomicUsize,
    host_pressure_latest_nominal_dispatch_seats: AtomicUsize,
    host_pressure_latest_effective_dispatch_seats: AtomicUsize,
    adaptive_controller_evaluations: AtomicU64,
    adaptive_controller_disabled_evaluations: AtomicU64,
    adaptive_controller_shadow_evaluations: AtomicU64,
    adaptive_controller_canary_evaluations: AtomicU64,
    adaptive_controller_live_evaluations: AtomicU64,
    adaptive_controller_rollback_evaluations: AtomicU64,
    adaptive_controller_decisions: AtomicU64,
    adaptive_controller_apply_target_decisions: AtomicU64,
    adaptive_controller_shadow_only_decisions: AtomicU64,
    adaptive_controller_canary_skipped_decisions: AtomicU64,
    adaptive_controller_rollback_to_static_decisions: AtomicU64,
    adaptive_controller_latest_recommended_warm_target_total: AtomicUsize,
    adaptive_controller_latest_effective_warm_target_total: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct RuntimeGlobalCountersSnapshot {
    pub active_runtime_instances: usize,
    pub queued_invocations: usize,
    pub worker_dispatched_invocations: u64,
    pub worker_affinity_routed_invocations: u64,
    pub worker_least_loaded_routed_invocations: u64,
    pub request_correlation_records: u64,
    pub request_correlation_nanos_total: u64,
    pub execution_plan_builds: u64,
    pub execution_plan_build_nanos_total: u64,
    pub admission_decisions: u64,
    pub admission_decision_nanos_total: u64,
    pub worker_router_dispatches: u64,
    pub worker_router_dispatch_nanos_total: u64,
    pub worker_affinity_cache_entries: usize,
    pub worker_affinity_cache_evictions: u64,
    pub retained_runtime_pool_entries: usize,
    pub retained_runtime_pool_evictions: u64,
    pub retained_runtime_pool_retirements: u64,
    pub bundle_loads: u64,
    pub bundle_load_nanos_total: u64,
    pub bundle_integrity_verifications: u64,
    pub bundle_integrity_verify_nanos_total: u64,
    pub bundle_module_loads: u64,
    pub bundle_module_load_nanos_total: u64,
    pub bundle_evaluations: u64,
    pub bundle_evaluation_nanos_total: u64,
    pub runtime_pool_hits: u64,
    pub runtime_pool_misses: u64,
    pub runtime_pool_replacements: u64,
    pub v8_startup_snapshot_runtime_constructions: u64,
    pub v8_unsnapshotted_runtime_constructions: u64,
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
    pub host_bridge_calls: u64,
    pub host_bridge_call_nanos_total: u64,
    pub nested_local_dispatches: u64,
    pub fallback_cross_runtime_dispatches: u64,
    pub warm_pool_hits: u64,
    pub warm_pool_misses: u64,
    pub warm_pool_retirements: u64,
    pub warm_pool_discard_unquiesced: u64,
    pub wasmtime_module_cache_hits: u64,
    pub wasmtime_module_cache_misses: u64,
    pub wasmtime_module_compilations: u64,
    pub wasmtime_module_compilation_nanos_total: u64,
    pub wasmtime_fuel_consumed_total: u64,
    pub wasmtime_fuel_exhaustions: u64,
    pub wasmtime_store_pool_hits: u64,
    pub wasmtime_store_pool_misses: u64,
    pub wasmtime_store_pool_authority_mismatches: u64,
    pub wasmtime_store_pool_evictions: u64,
    pub wasmtime_store_pool_retirements: u64,
    pub host_pressure: RuntimeHostPressureMetricsSnapshot,
    pub adaptive_controller: RuntimeAdaptiveControllerMetricsSnapshot,
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

    pub(super) fn increment_active_runtime_instances(&self) {
        self.active_runtime_instances
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

    pub(super) fn record_request_correlation(&self, duration: Duration) {
        self.request_correlation_records
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.request_correlation_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_execution_plan_build(&self, duration: Duration) {
        self.execution_plan_builds
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.execution_plan_build_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_admission_decision(&self, duration: Duration) {
        self.admission_decisions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.admission_decision_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_worker_router_dispatch(&self, duration: Duration) {
        self.worker_router_dispatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.worker_router_dispatch_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
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

    pub(super) fn record_bundle_load(&self, duration: Duration) {
        self.bundle_loads.fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.bundle_load_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_bundle_integrity_verify(&self, duration: Duration) {
        self.bundle_integrity_verifications
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.bundle_integrity_verify_nanos_total
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

    pub(super) fn record_runtime_pool_hit(&self) {
        self.runtime_pool_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_runtime_pool_miss(&self) {
        self.runtime_pool_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_runtime_pool_replacement(&self) {
        self.runtime_pool_replacements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_v8_startup_snapshot_runtime_construction(&self) {
        self.v8_startup_snapshot_runtime_constructions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_v8_unsnapshotted_runtime_construction(&self) {
        self.v8_unsnapshotted_runtime_constructions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn decrement_active_runtime_instances(&self) {
        self.active_runtime_instances
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

    pub(super) fn record_host_bridge_call(&self, duration: Duration) {
        self.host_bridge_calls
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.host_bridge_call_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_nested_local_dispatch(&self) {
        self.nested_local_dispatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_fallback_cross_runtime_dispatch(&self) {
        self.fallback_cross_runtime_dispatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_warm_pool_hit(&self) {
        self.warm_pool_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_warm_pool_miss(&self) {
        self.warm_pool_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_warm_pool_retirement(&self) {
        self.warm_pool_retirements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_warm_pool_discard_unquiesced(&self) {
        self.warm_pool_discard_unquiesced
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_module_cache_hit(&self) {
        self.wasmtime_module_cache_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_module_cache_miss(&self) {
        self.wasmtime_module_cache_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_module_compilation_time(&self, duration: Duration) {
        self.wasmtime_module_compilations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        self.wasmtime_module_compilation_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_fuel_consumed(&self, fuel: u64) {
        self.wasmtime_fuel_consumed_total
            .fetch_add(fuel, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_fuel_exhaustion(&self) {
        self.wasmtime_fuel_exhaustions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_store_pool_hit(&self) {
        self.wasmtime_store_pool_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_store_pool_miss(&self) {
        self.wasmtime_store_pool_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_store_pool_authority_mismatch(&self) {
        self.wasmtime_store_pool_authority_mismatches
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_store_pool_eviction(&self) {
        self.wasmtime_store_pool_evictions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_wasmtime_store_pool_retirement(&self) {
        self.wasmtime_store_pool_retirements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_host_resource_decision(&self, decision: RuntimeHostResourceDecision) {
        self.host_pressure_decisions
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        match decision.host_pressure_level {
            RuntimeHostPressureLevel::Nominal => {
                self.host_pressure_nominal_decisions
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
            RuntimeHostPressureLevel::High => {
                self.host_pressure_high_decisions
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
            RuntimeHostPressureLevel::Critical => {
                self.host_pressure_critical_decisions
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
        }
        if matches!(
            decision.cpu_source_status,
            RuntimeHostPressureSourceStatus::Unavailable
        ) {
            self.host_pressure_cpu_source_unavailable_decisions
                .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        }
        if matches!(
            decision.memory_source_status,
            RuntimeMemoryPressureSourceStatus::Unavailable
        ) {
            self.host_pressure_memory_source_unavailable_decisions
                .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        }
        self.host_pressure_latest_host_level.store(
            encode_host_pressure_level(decision.host_pressure_level),
            DIAGNOSTIC_COUNTER_ORDERING,
        );
        self.host_pressure_latest_cpu_level.store(
            encode_host_pressure_level(decision.cpu_pressure_level),
            DIAGNOSTIC_COUNTER_ORDERING,
        );
        self.host_pressure_latest_cpu_source_status.store(
            encode_host_pressure_source_status(decision.cpu_source_status),
            DIAGNOSTIC_COUNTER_ORDERING,
        );
        self.host_pressure_latest_memory_level.store(
            encode_memory_pressure_level(decision.memory_pressure_level),
            DIAGNOSTIC_COUNTER_ORDERING,
        );
        self.host_pressure_latest_memory_source_status.store(
            encode_memory_pressure_source_status(decision.memory_source_status),
            DIAGNOSTIC_COUNTER_ORDERING,
        );
        self.host_pressure_latest_nominal_dispatch_seats
            .store(decision.nominal_dispatch_seats, DIAGNOSTIC_COUNTER_ORDERING);
        self.host_pressure_latest_effective_dispatch_seats.store(
            decision.effective_dispatch_seats,
            DIAGNOSTIC_COUNTER_ORDERING,
        );
    }

    pub(super) fn record_adaptive_controller_evaluation(
        &self,
        evaluation: &RuntimeAdaptiveWarmPoolEvaluation,
    ) {
        self.adaptive_controller_evaluations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        match evaluation.mode {
            RuntimeAdaptiveControllerMode::Disabled => {
                self.adaptive_controller_disabled_evaluations
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
            RuntimeAdaptiveControllerMode::Shadow => {
                self.adaptive_controller_shadow_evaluations
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
            RuntimeAdaptiveControllerMode::Canary => {
                self.adaptive_controller_canary_evaluations
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
            RuntimeAdaptiveControllerMode::Live => {
                self.adaptive_controller_live_evaluations
                    .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            }
        }
        if evaluation.rollback_to_static_defaults {
            self.adaptive_controller_rollback_evaluations
                .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
        }

        let mut recommended_total = 0usize;
        let mut effective_total = 0usize;
        for decision in &evaluation.decisions {
            self.adaptive_controller_decisions
                .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
            recommended_total = recommended_total.saturating_add(decision.recommended_warm_target);
            effective_total = effective_total.saturating_add(decision.effective_warm_target);
            match decision.actuation.kind {
                RuntimeAdaptiveWarmPoolActuationKind::ApplyTarget => {
                    self.adaptive_controller_apply_target_decisions
                        .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
                }
                RuntimeAdaptiveWarmPoolActuationKind::ShadowOnly => {
                    self.adaptive_controller_shadow_only_decisions
                        .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
                }
                RuntimeAdaptiveWarmPoolActuationKind::CanarySkipped => {
                    self.adaptive_controller_canary_skipped_decisions
                        .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
                }
                RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic => {
                    self.adaptive_controller_rollback_to_static_decisions
                        .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
                }
                RuntimeAdaptiveWarmPoolActuationKind::NoopDisabled => {}
            }
        }
        self.adaptive_controller_latest_recommended_warm_target_total
            .store(recommended_total, DIAGNOSTIC_COUNTER_ORDERING);
        self.adaptive_controller_latest_effective_warm_target_total
            .store(effective_total, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn snapshot(&self) -> RuntimeGlobalCountersSnapshot {
        RuntimeGlobalCountersSnapshot {
            active_runtime_instances: self
                .active_runtime_instances
                .load(DIAGNOSTIC_COUNTER_ORDERING),
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
            request_correlation_records: self
                .request_correlation_records
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            request_correlation_nanos_total: self
                .request_correlation_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            execution_plan_builds: self.execution_plan_builds.load(DIAGNOSTIC_COUNTER_ORDERING),
            execution_plan_build_nanos_total: self
                .execution_plan_build_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            admission_decisions: self.admission_decisions.load(DIAGNOSTIC_COUNTER_ORDERING),
            admission_decision_nanos_total: self
                .admission_decision_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_router_dispatches: self
                .worker_router_dispatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            worker_router_dispatch_nanos_total: self
                .worker_router_dispatch_nanos_total
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
            bundle_loads: self.bundle_loads.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_load_nanos_total: self
                .bundle_load_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_integrity_verifications: self
                .bundle_integrity_verifications
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_integrity_verify_nanos_total: self
                .bundle_integrity_verify_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_module_loads: self.bundle_module_loads.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_module_load_nanos_total: self
                .bundle_module_load_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_evaluations: self.bundle_evaluations.load(DIAGNOSTIC_COUNTER_ORDERING),
            bundle_evaluation_nanos_total: self
                .bundle_evaluation_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_hits: self.runtime_pool_hits.load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_misses: self.runtime_pool_misses.load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_replacements: self
                .runtime_pool_replacements
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            v8_startup_snapshot_runtime_constructions: self
                .v8_startup_snapshot_runtime_constructions
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            v8_unsnapshotted_runtime_constructions: self
                .v8_unsnapshotted_runtime_constructions
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
            host_bridge_calls: self.host_bridge_calls.load(DIAGNOSTIC_COUNTER_ORDERING),
            host_bridge_call_nanos_total: self
                .host_bridge_call_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            nested_local_dispatches: self
                .nested_local_dispatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            fallback_cross_runtime_dispatches: self
                .fallback_cross_runtime_dispatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            warm_pool_hits: self.warm_pool_hits.load(DIAGNOSTIC_COUNTER_ORDERING),
            warm_pool_misses: self.warm_pool_misses.load(DIAGNOSTIC_COUNTER_ORDERING),
            warm_pool_retirements: self.warm_pool_retirements.load(DIAGNOSTIC_COUNTER_ORDERING),
            warm_pool_discard_unquiesced: self
                .warm_pool_discard_unquiesced
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_module_cache_hits: self
                .wasmtime_module_cache_hits
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_module_cache_misses: self
                .wasmtime_module_cache_misses
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_module_compilations: self
                .wasmtime_module_compilations
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_module_compilation_nanos_total: self
                .wasmtime_module_compilation_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_fuel_consumed_total: self
                .wasmtime_fuel_consumed_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_fuel_exhaustions: self
                .wasmtime_fuel_exhaustions
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_store_pool_hits: self
                .wasmtime_store_pool_hits
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_store_pool_misses: self
                .wasmtime_store_pool_misses
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_store_pool_authority_mismatches: self
                .wasmtime_store_pool_authority_mismatches
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_store_pool_evictions: self
                .wasmtime_store_pool_evictions
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            wasmtime_store_pool_retirements: self
                .wasmtime_store_pool_retirements
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            host_pressure: RuntimeHostPressureMetricsSnapshot {
                decisions: self
                    .host_pressure_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                nominal_decisions: self
                    .host_pressure_nominal_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                high_decisions: self
                    .host_pressure_high_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                critical_decisions: self
                    .host_pressure_critical_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                cpu_source_unavailable_decisions: self
                    .host_pressure_cpu_source_unavailable_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                memory_source_unavailable_decisions: self
                    .host_pressure_memory_source_unavailable_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                latest_host_pressure_level: decode_host_pressure_level(
                    self.host_pressure_latest_host_level
                        .load(DIAGNOSTIC_COUNTER_ORDERING),
                ),
                latest_cpu_pressure_level: decode_host_pressure_level(
                    self.host_pressure_latest_cpu_level
                        .load(DIAGNOSTIC_COUNTER_ORDERING),
                ),
                latest_cpu_source_status: decode_host_pressure_source_status(
                    self.host_pressure_latest_cpu_source_status
                        .load(DIAGNOSTIC_COUNTER_ORDERING),
                ),
                latest_memory_pressure_level: decode_memory_pressure_level(
                    self.host_pressure_latest_memory_level
                        .load(DIAGNOSTIC_COUNTER_ORDERING),
                ),
                latest_memory_source_status: decode_memory_pressure_source_status(
                    self.host_pressure_latest_memory_source_status
                        .load(DIAGNOSTIC_COUNTER_ORDERING),
                ),
                latest_nominal_dispatch_seats: self
                    .host_pressure_latest_nominal_dispatch_seats
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                latest_effective_dispatch_seats: self
                    .host_pressure_latest_effective_dispatch_seats
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
            },
            adaptive_controller: RuntimeAdaptiveControllerMetricsSnapshot {
                evaluations: self
                    .adaptive_controller_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                disabled_evaluations: self
                    .adaptive_controller_disabled_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                shadow_evaluations: self
                    .adaptive_controller_shadow_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                canary_evaluations: self
                    .adaptive_controller_canary_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                live_evaluations: self
                    .adaptive_controller_live_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                rollback_evaluations: self
                    .adaptive_controller_rollback_evaluations
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                decisions: self
                    .adaptive_controller_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                apply_target_decisions: self
                    .adaptive_controller_apply_target_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                shadow_only_decisions: self
                    .adaptive_controller_shadow_only_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                canary_skipped_decisions: self
                    .adaptive_controller_canary_skipped_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                rollback_to_static_decisions: self
                    .adaptive_controller_rollback_to_static_decisions
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                latest_recommended_warm_target_total: self
                    .adaptive_controller_latest_recommended_warm_target_total
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
                latest_effective_warm_target_total: self
                    .adaptive_controller_latest_effective_warm_target_total
                    .load(DIAGNOSTIC_COUNTER_ORDERING),
            },
        }
    }
}

fn encode_host_pressure_level(level: RuntimeHostPressureLevel) -> usize {
    match level {
        RuntimeHostPressureLevel::Nominal => 0,
        RuntimeHostPressureLevel::High => 1,
        RuntimeHostPressureLevel::Critical => 2,
    }
}

fn decode_host_pressure_level(value: usize) -> RuntimeHostPressureLevel {
    match value {
        1 => RuntimeHostPressureLevel::High,
        2 => RuntimeHostPressureLevel::Critical,
        _ => RuntimeHostPressureLevel::Nominal,
    }
}

fn encode_host_pressure_source_status(status: RuntimeHostPressureSourceStatus) -> usize {
    match status {
        RuntimeHostPressureSourceStatus::Observed => 0,
        RuntimeHostPressureSourceStatus::Unavailable => 1,
    }
}

fn decode_host_pressure_source_status(value: usize) -> RuntimeHostPressureSourceStatus {
    match value {
        1 => RuntimeHostPressureSourceStatus::Unavailable,
        _ => RuntimeHostPressureSourceStatus::Observed,
    }
}

fn encode_memory_pressure_level(level: RuntimeMemoryPressureLevel) -> usize {
    match level {
        RuntimeMemoryPressureLevel::Nominal => 0,
        RuntimeMemoryPressureLevel::High => 1,
        RuntimeMemoryPressureLevel::Critical => 2,
    }
}

fn decode_memory_pressure_level(value: usize) -> RuntimeMemoryPressureLevel {
    match value {
        1 => RuntimeMemoryPressureLevel::High,
        2 => RuntimeMemoryPressureLevel::Critical,
        _ => RuntimeMemoryPressureLevel::Nominal,
    }
}

fn encode_memory_pressure_source_status(status: RuntimeMemoryPressureSourceStatus) -> usize {
    match status {
        RuntimeMemoryPressureSourceStatus::Observed => 0,
        RuntimeMemoryPressureSourceStatus::Unavailable => 1,
    }
}

fn decode_memory_pressure_source_status(value: usize) -> RuntimeMemoryPressureSourceStatus {
    match value {
        1 => RuntimeMemoryPressureSourceStatus::Unavailable,
        _ => RuntimeMemoryPressureSourceStatus::Observed,
    }
}
