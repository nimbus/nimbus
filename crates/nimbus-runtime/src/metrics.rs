mod correlations;
mod global;
mod host_operations;
mod profiles;
mod tenants;

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Serialize;

use crate::context::RuntimeInvocationContext;
use crate::host::HostCallCancellationCause;
use crate::limits::{
    RuntimeAdaptiveMetricsSink, RuntimeAdaptiveWarmPoolEvaluation, RuntimeHostPressureLevel,
    RuntimeHostPressureSourceStatus, RuntimeHostResourceDecision, RuntimeMemoryPressureLevel,
    RuntimeMemoryPressureSourceStatus, RuntimeProfile,
};

pub use self::correlations::RuntimeRequestCorrelationSnapshot;
use self::global::RuntimeGlobalCounters;
pub use self::host_operations::RuntimeHostOperationMetricsSnapshot;
use self::host_operations::RuntimeHostOperationRegistry;
use self::profiles::RuntimeProfileTelemetryRegistry;
pub use self::profiles::{RuntimeProfileCountersSnapshot, RuntimeProfileTelemetrySnapshot};
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
    profiles: RuntimeProfileTelemetryRegistry,
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
    pub fresh_realm_creates: u64,
    pub fresh_realm_create_nanos_total: u64,
    pub fresh_realm_bootstrap_installs: u64,
    pub fresh_realm_bootstrap_install_nanos_total: u64,
    pub fresh_realm_bootstrap_finalizes: u64,
    pub fresh_realm_bootstrap_finalize_nanos_total: u64,
    pub fresh_realm_bootstrap_resets: u64,
    pub fresh_realm_bootstrap_reset_nanos_total: u64,
    pub fresh_realm_invocation_scripts: u64,
    pub fresh_realm_invocation_script_nanos_total: u64,
    pub fresh_realm_promise_resolves: u64,
    pub fresh_realm_promise_resolve_nanos_total: u64,
    pub fresh_realm_deserializations: u64,
    pub fresh_realm_deserialization_nanos_total: u64,
    pub fresh_realm_destroys: u64,
    pub fresh_realm_destroy_nanos_total: u64,
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
    pub profiles: RuntimeProfileTelemetrySnapshot,
    pub host_operations: std::collections::BTreeMap<String, RuntimeHostOperationMetricsSnapshot>,
    pub tenants: std::collections::BTreeMap<String, RuntimeTenantMetricsSnapshot>,
    pub recent_request_correlations: Vec<RuntimeRequestCorrelationSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeHostPressureMetricsSnapshot {
    pub decisions: u64,
    pub nominal_decisions: u64,
    pub high_decisions: u64,
    pub critical_decisions: u64,
    pub cpu_source_unavailable_decisions: u64,
    pub memory_source_unavailable_decisions: u64,
    pub latest_host_pressure_level: RuntimeHostPressureLevel,
    pub latest_cpu_pressure_level: RuntimeHostPressureLevel,
    pub latest_cpu_source_status: RuntimeHostPressureSourceStatus,
    pub latest_memory_pressure_level: RuntimeMemoryPressureLevel,
    pub latest_memory_source_status: RuntimeMemoryPressureSourceStatus,
    pub latest_nominal_dispatch_seats: usize,
    pub latest_effective_dispatch_seats: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct RuntimeAdaptiveControllerMetricsSnapshot {
    pub evaluations: u64,
    pub disabled_evaluations: u64,
    pub shadow_evaluations: u64,
    pub canary_evaluations: u64,
    pub live_evaluations: u64,
    pub rollback_evaluations: u64,
    pub decisions: u64,
    pub apply_target_decisions: u64,
    pub shadow_only_decisions: u64,
    pub canary_skipped_decisions: u64,
    pub rollback_to_static_decisions: u64,
    pub latest_recommended_warm_target_total: usize,
    pub latest_effective_warm_target_total: usize,
}

impl Default for RuntimeHostPressureMetricsSnapshot {
    fn default() -> Self {
        Self {
            decisions: 0,
            nominal_decisions: 0,
            high_decisions: 0,
            critical_decisions: 0,
            cpu_source_unavailable_decisions: 0,
            memory_source_unavailable_decisions: 0,
            latest_host_pressure_level: RuntimeHostPressureLevel::Nominal,
            latest_cpu_pressure_level: RuntimeHostPressureLevel::Nominal,
            latest_cpu_source_status: RuntimeHostPressureSourceStatus::Observed,
            latest_memory_pressure_level: RuntimeMemoryPressureLevel::Nominal,
            latest_memory_source_status: RuntimeMemoryPressureSourceStatus::Observed,
            latest_nominal_dispatch_seats: 0,
            latest_effective_dispatch_seats: 0,
        }
    }
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

    pub fn record_profile_invocation_started(&self, profile: Option<RuntimeProfile>) {
        self.profiles.record_invocation_started(profile);
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

    pub fn record_request_correlation_duration(&self, duration: Duration) {
        self.global.record_request_correlation(duration);
    }

    pub fn record_execution_plan_build(&self, duration: Duration) {
        self.global.record_execution_plan_build(duration);
    }

    pub fn record_admission_decision(&self, duration: Duration) {
        self.global.record_admission_decision(duration);
    }

    pub fn record_worker_router_dispatch(&self, duration: Duration) {
        self.global.record_worker_router_dispatch(duration);
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

    pub fn record_wasmtime_module_cache_hit(&self) {
        self.global.record_wasmtime_module_cache_hit();
    }

    pub fn record_wasmtime_module_cache_miss(&self) {
        self.global.record_wasmtime_module_cache_miss();
    }

    pub fn record_wasmtime_module_compilation_time(&self, duration: Duration) {
        self.global
            .record_wasmtime_module_compilation_time(duration);
    }

    pub fn record_wasmtime_fuel_consumed(&self, fuel: u64) {
        self.global.record_wasmtime_fuel_consumed(fuel);
    }

    pub fn record_wasmtime_fuel_exhaustion(&self) {
        self.global.record_wasmtime_fuel_exhaustion();
    }

    pub fn record_wasmtime_store_pool_hit(&self) {
        self.global.record_wasmtime_store_pool_hit();
    }

    pub fn record_wasmtime_store_pool_miss(&self) {
        self.global.record_wasmtime_store_pool_miss();
    }

    pub fn record_wasmtime_store_pool_authority_mismatch(&self) {
        self.global.record_wasmtime_store_pool_authority_mismatch();
    }

    pub fn record_wasmtime_store_pool_eviction(&self) {
        self.global.record_wasmtime_store_pool_eviction();
    }

    pub fn record_wasmtime_store_pool_retirement(&self) {
        self.global.record_wasmtime_store_pool_retirement();
    }

    pub fn record_bundle_load(&self, duration: Duration) {
        self.global.record_bundle_load(duration);
    }

    pub fn record_bundle_integrity_verify(&self, duration: Duration) {
        self.global.record_bundle_integrity_verify(duration);
    }

    pub fn record_bundle_module_load(&self, duration: Duration) {
        self.global.record_bundle_module_load(duration);
    }

    pub fn record_bundle_evaluation(&self, duration: Duration) {
        self.global.record_bundle_evaluation(duration);
    }

    pub fn record_fresh_realm_create(&self, duration: Duration) {
        self.global.record_fresh_realm_create(duration);
    }

    pub fn record_fresh_realm_bootstrap_install(&self, duration: Duration) {
        self.global.record_fresh_realm_bootstrap_install(duration);
    }

    pub fn record_fresh_realm_bootstrap_finalize(&self, duration: Duration) {
        self.global.record_fresh_realm_bootstrap_finalize(duration);
    }

    pub fn record_fresh_realm_bootstrap_reset(&self, duration: Duration) {
        self.global.record_fresh_realm_bootstrap_reset(duration);
    }

    pub fn record_fresh_realm_invocation_script(&self, duration: Duration) {
        self.global.record_fresh_realm_invocation_script(duration);
    }

    pub fn record_fresh_realm_promise_resolve(&self, duration: Duration) {
        self.global.record_fresh_realm_promise_resolve(duration);
    }

    pub fn record_fresh_realm_deserialization(&self, duration: Duration) {
        self.global.record_fresh_realm_deserialization(duration);
    }

    pub fn record_fresh_realm_destroy(&self, duration: Duration) {
        self.global.record_fresh_realm_destroy(duration);
    }

    pub fn record_runtime_pool_hit(&self) {
        self.global.record_runtime_pool_hit();
    }

    pub fn record_profile_runtime_pool_hit(&self, profile: Option<RuntimeProfile>) {
        self.profiles.record_runtime_pool_hit(profile);
    }

    pub fn record_runtime_pool_miss(&self) {
        self.global.record_runtime_pool_miss();
    }

    pub fn record_profile_runtime_pool_miss(&self, profile: Option<RuntimeProfile>) {
        self.profiles.record_runtime_pool_miss(profile);
    }

    pub fn record_runtime_pool_replacement(&self) {
        self.global.record_runtime_pool_replacement();
    }

    pub fn record_profile_runtime_pool_replacement(&self, profile: Option<RuntimeProfile>) {
        self.profiles.record_runtime_pool_replacement(profile);
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

    pub fn record_profile_invocation_completed(&self, profile: Option<RuntimeProfile>) {
        self.profiles.record_invocation_completed(profile);
    }

    pub fn record_queue_wait(&self, duration: Duration) {
        self.record_queue_wait_for_tenant(None, duration);
    }

    pub fn record_queue_wait_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.global.record_queue_wait(duration);
        self.tenants.record_queue_wait(tenant_label, duration);
    }

    pub fn record_profile_queue_wait(&self, profile: Option<RuntimeProfile>, duration: Duration) {
        self.profiles.record_queue_wait(profile, duration);
    }

    pub fn record_execution(&self, duration: Duration) {
        self.record_execution_for_tenant(None, duration);
    }

    pub fn record_execution_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.global.record_execution(duration);
        self.tenants.record_execution(tenant_label, duration);
    }

    pub fn record_profile_execution(&self, profile: Option<RuntimeProfile>, duration: Duration) {
        self.profiles.record_execution(profile, duration);
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

    pub fn record_host_bridge_call(&self, duration: Duration) {
        self.global.record_host_bridge_call(duration);
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

    pub fn record_host_operation_duration(&self, operation: &str, duration: Duration) {
        self.host_operations.record_duration(operation, duration);
    }

    pub fn record_nested_local_dispatch(&self) {
        self.global.record_nested_local_dispatch();
    }

    pub fn record_fallback_cross_runtime_dispatch(&self) {
        self.global.record_fallback_cross_runtime_dispatch();
    }

    pub fn record_host_resource_decision(&self, decision: RuntimeHostResourceDecision) {
        self.global.record_host_resource_decision(decision);
    }

    pub fn record_adaptive_controller_evaluation(
        &self,
        evaluation: &RuntimeAdaptiveWarmPoolEvaluation,
    ) {
        self.global
            .record_adaptive_controller_evaluation(evaluation);
    }

    pub fn record_request_correlation(&self, context: &RuntimeInvocationContext) {
        let started_at = std::time::Instant::now();
        self.recent_request_correlations.record(context);
        self.record_request_correlation_duration(started_at.elapsed());
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let global = self.global.snapshot();
        RuntimeMetricsSnapshot {
            active_runtime_instances: global.active_runtime_instances,
            queued_invocations: global.queued_invocations,
            worker_dispatched_invocations: global.worker_dispatched_invocations,
            worker_affinity_routed_invocations: global.worker_affinity_routed_invocations,
            worker_least_loaded_routed_invocations: global.worker_least_loaded_routed_invocations,
            request_correlation_records: global.request_correlation_records,
            request_correlation_nanos_total: global.request_correlation_nanos_total,
            execution_plan_builds: global.execution_plan_builds,
            execution_plan_build_nanos_total: global.execution_plan_build_nanos_total,
            admission_decisions: global.admission_decisions,
            admission_decision_nanos_total: global.admission_decision_nanos_total,
            worker_router_dispatches: global.worker_router_dispatches,
            worker_router_dispatch_nanos_total: global.worker_router_dispatch_nanos_total,
            worker_affinity_cache_entries: global.worker_affinity_cache_entries,
            worker_affinity_cache_evictions: global.worker_affinity_cache_evictions,
            retained_runtime_pool_entries: global.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: global.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: global.retained_runtime_pool_retirements,
            bundle_loads: global.bundle_loads,
            bundle_load_nanos_total: global.bundle_load_nanos_total,
            bundle_integrity_verifications: global.bundle_integrity_verifications,
            bundle_integrity_verify_nanos_total: global.bundle_integrity_verify_nanos_total,
            bundle_module_loads: global.bundle_module_loads,
            bundle_module_load_nanos_total: global.bundle_module_load_nanos_total,
            bundle_evaluations: global.bundle_evaluations,
            bundle_evaluation_nanos_total: global.bundle_evaluation_nanos_total,
            fresh_realm_creates: global.fresh_realm_creates,
            fresh_realm_create_nanos_total: global.fresh_realm_create_nanos_total,
            fresh_realm_bootstrap_installs: global.fresh_realm_bootstrap_installs,
            fresh_realm_bootstrap_install_nanos_total: global
                .fresh_realm_bootstrap_install_nanos_total,
            fresh_realm_bootstrap_finalizes: global.fresh_realm_bootstrap_finalizes,
            fresh_realm_bootstrap_finalize_nanos_total: global
                .fresh_realm_bootstrap_finalize_nanos_total,
            fresh_realm_bootstrap_resets: global.fresh_realm_bootstrap_resets,
            fresh_realm_bootstrap_reset_nanos_total: global.fresh_realm_bootstrap_reset_nanos_total,
            fresh_realm_invocation_scripts: global.fresh_realm_invocation_scripts,
            fresh_realm_invocation_script_nanos_total: global
                .fresh_realm_invocation_script_nanos_total,
            fresh_realm_promise_resolves: global.fresh_realm_promise_resolves,
            fresh_realm_promise_resolve_nanos_total: global.fresh_realm_promise_resolve_nanos_total,
            fresh_realm_deserializations: global.fresh_realm_deserializations,
            fresh_realm_deserialization_nanos_total: global.fresh_realm_deserialization_nanos_total,
            fresh_realm_destroys: global.fresh_realm_destroys,
            fresh_realm_destroy_nanos_total: global.fresh_realm_destroy_nanos_total,
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
            host_bridge_calls: global.host_bridge_calls,
            host_bridge_call_nanos_total: global.host_bridge_call_nanos_total,
            nested_local_dispatches: global.nested_local_dispatches,
            fallback_cross_runtime_dispatches: global.fallback_cross_runtime_dispatches,
            warm_pool_hits: global.warm_pool_hits,
            warm_pool_misses: global.warm_pool_misses,
            warm_pool_retirements: global.warm_pool_retirements,
            warm_pool_discard_unquiesced: global.warm_pool_discard_unquiesced,
            wasmtime_module_cache_hits: global.wasmtime_module_cache_hits,
            wasmtime_module_cache_misses: global.wasmtime_module_cache_misses,
            wasmtime_module_compilations: global.wasmtime_module_compilations,
            wasmtime_module_compilation_nanos_total: global.wasmtime_module_compilation_nanos_total,
            wasmtime_fuel_consumed_total: global.wasmtime_fuel_consumed_total,
            wasmtime_fuel_exhaustions: global.wasmtime_fuel_exhaustions,
            wasmtime_store_pool_hits: global.wasmtime_store_pool_hits,
            wasmtime_store_pool_misses: global.wasmtime_store_pool_misses,
            wasmtime_store_pool_authority_mismatches: global
                .wasmtime_store_pool_authority_mismatches,
            wasmtime_store_pool_evictions: global.wasmtime_store_pool_evictions,
            wasmtime_store_pool_retirements: global.wasmtime_store_pool_retirements,
            host_pressure: global.host_pressure,
            adaptive_controller: global.adaptive_controller,
            profiles: self.profiles.snapshot(),
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

impl RuntimeAdaptiveMetricsSink for RuntimeMetrics {
    fn record_controller_evaluation(&self, evaluation: &RuntimeAdaptiveWarmPoolEvaluation) {
        self.record_adaptive_controller_evaluation(evaluation);
    }
}

pub(super) fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use super::*;
    use crate::limits::{
        RuntimeAdaptiveControllerMode, RuntimeAdaptiveControllerSettings,
        RuntimeAdaptiveWarmPoolAuthorityInput, RuntimeAdaptiveWarmPoolController,
        RuntimeAdaptiveWarmPoolSnapshot, RuntimeControllerReplayAuthorityInput,
        RuntimeControllerReplayAuthorityKey, RuntimeControllerReplayConfig,
        RuntimeControllerReplayObservation, RuntimeControllerReplayState,
    };

    #[test]
    fn host_pressure_metrics_snapshot_is_low_cardinality_global_state() {
        let metrics = RuntimeMetrics::default();

        metrics.record_host_resource_decision(host_resource_decision(
            RuntimeHostPressureLevel::High,
            RuntimeHostPressureLevel::High,
            RuntimeHostPressureSourceStatus::Unavailable,
            RuntimeMemoryPressureLevel::Nominal,
            RuntimeMemoryPressureSourceStatus::Observed,
            4,
            2,
        ));
        metrics.record_host_resource_decision(host_resource_decision(
            RuntimeHostPressureLevel::Critical,
            RuntimeHostPressureLevel::High,
            RuntimeHostPressureSourceStatus::Observed,
            RuntimeMemoryPressureLevel::Critical,
            RuntimeMemoryPressureSourceStatus::Unavailable,
            4,
            0,
        ));

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.host_pressure.decisions, 2);
        assert_eq!(snapshot.host_pressure.nominal_decisions, 0);
        assert_eq!(snapshot.host_pressure.high_decisions, 1);
        assert_eq!(snapshot.host_pressure.critical_decisions, 1);
        assert_eq!(snapshot.host_pressure.cpu_source_unavailable_decisions, 1);
        assert_eq!(
            snapshot.host_pressure.memory_source_unavailable_decisions,
            1
        );
        assert_eq!(
            snapshot.host_pressure.latest_host_pressure_level,
            RuntimeHostPressureLevel::Critical
        );
        assert_eq!(
            snapshot.host_pressure.latest_memory_pressure_level,
            RuntimeMemoryPressureLevel::Critical
        );
        assert_eq!(snapshot.host_pressure.latest_nominal_dispatch_seats, 4);
        assert_eq!(snapshot.host_pressure.latest_effective_dispatch_seats, 0);
        assert!(
            snapshot.tenants.is_empty(),
            "host pressure metrics must not add tenant-cardinality labels"
        );
    }

    #[test]
    fn adaptive_controller_metrics_snapshot_is_low_cardinality_global_state() {
        let metrics = RuntimeMetrics::default();
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::shadow(RuntimeControllerReplayConfig {
                stable_window_observations: nonzero_usize(2),
                panic_window_observations: nonzero_usize(1),
                max_scale_up_step: nonzero_usize(16),
                max_scale_down_step: nonzero_usize(16),
                scale_down_hysteresis_observations: 0,
                max_warm_runtimes_per_authority: 16,
                max_warm_runtimes_per_tenant: 16,
                ..RuntimeControllerReplayConfig::default()
            }),
        );
        let evaluation = controller.evaluate_snapshot(RuntimeAdaptiveWarmPoolSnapshot {
            observed_at_millis: 7,
            host_resource_decision: host_resource_decision(
                RuntimeHostPressureLevel::Nominal,
                RuntimeHostPressureLevel::Nominal,
                RuntimeHostPressureSourceStatus::Observed,
                RuntimeMemoryPressureLevel::Nominal,
                RuntimeMemoryPressureSourceStatus::Observed,
                4,
                4,
            ),
            authorities: vec![RuntimeAdaptiveWarmPoolAuthorityInput {
                replay_input: RuntimeControllerReplayAuthorityInput {
                    key: RuntimeControllerReplayAuthorityKey {
                        tenant_hash: 11,
                        authority_hash: 22,
                        profile: RuntimeProfile::WebLean,
                    },
                    previous_state: RuntimeControllerReplayState {
                        current_warm_target: 1,
                        scale_down_observations_remaining: 0,
                    },
                    observations: vec![RuntimeControllerReplayObservation::nominal(
                        4, 2_000_000, 200_000,
                    )],
                },
                static_warm_target: 1,
                current_retained_runtimes: 1,
                projected_bytes_per_runtime: 128 * 1024 * 1024,
            }],
        });

        metrics.record_adaptive_controller_evaluation(&evaluation);
        let snapshot = metrics.snapshot();

        assert_eq!(evaluation.mode, RuntimeAdaptiveControllerMode::Shadow);
        assert_eq!(snapshot.adaptive_controller.evaluations, 1);
        assert_eq!(snapshot.adaptive_controller.shadow_evaluations, 1);
        assert_eq!(snapshot.adaptive_controller.decisions, 1);
        assert_eq!(snapshot.adaptive_controller.shadow_only_decisions, 1);
        assert_eq!(
            snapshot
                .adaptive_controller
                .latest_recommended_warm_target_total,
            3
        );
        assert_eq!(
            snapshot
                .adaptive_controller
                .latest_effective_warm_target_total,
            1
        );
        assert!(
            snapshot.tenants.is_empty(),
            "adaptive controller metrics must not add tenant-cardinality labels"
        );
    }

    #[test]
    fn profile_metrics_snapshot_is_fixed_bucket_runtime_state() {
        let metrics = RuntimeMetrics::default();

        metrics.record_profile_invocation_started(Some(RuntimeProfile::WebLean));
        metrics.record_profile_queue_wait(Some(RuntimeProfile::WebLean), Duration::from_micros(9));
        metrics.record_profile_execution(Some(RuntimeProfile::WebLean), Duration::from_millis(3));
        metrics.record_profile_runtime_pool_miss(Some(RuntimeProfile::WebLean));
        metrics.record_profile_invocation_completed(Some(RuntimeProfile::WebLean));

        metrics.record_profile_invocation_started(Some(RuntimeProfile::NodeFull));
        metrics.record_profile_runtime_pool_hit(Some(RuntimeProfile::NodeFull));
        metrics.record_profile_runtime_pool_replacement(Some(RuntimeProfile::NodeFull));

        metrics.record_profile_invocation_started(None);
        metrics.record_profile_queue_wait(None, Duration::from_millis(1));
        metrics.record_profile_execution(None, Duration::from_millis(2));
        metrics.record_profile_runtime_pool_miss(None);
        metrics.record_profile_invocation_completed(None);

        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot.profiles,
            RuntimeProfileTelemetrySnapshot {
                web_lean: RuntimeProfileCountersSnapshot {
                    started_invocations: 1,
                    completed_invocations: 1,
                    queue_wait_nanos_total: 9_000,
                    execution_nanos_total: 3_000_000,
                    runtime_pool_hits: 0,
                    runtime_pool_misses: 1,
                    runtime_pool_replacements: 0,
                },
                node_full: RuntimeProfileCountersSnapshot {
                    started_invocations: 1,
                    completed_invocations: 0,
                    queue_wait_nanos_total: 0,
                    execution_nanos_total: 0,
                    runtime_pool_hits: 1,
                    runtime_pool_misses: 0,
                    runtime_pool_replacements: 1,
                },
                unprofiled: RuntimeProfileCountersSnapshot {
                    started_invocations: 1,
                    completed_invocations: 1,
                    queue_wait_nanos_total: 1_000_000,
                    execution_nanos_total: 2_000_000,
                    runtime_pool_hits: 0,
                    runtime_pool_misses: 1,
                    runtime_pool_replacements: 0,
                },
            }
        );
        assert!(
            snapshot.tenants.is_empty(),
            "profile metrics must not add tenant-cardinality labels"
        );
    }

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
                is_top_level: true,
                bypasses_concurrency_limit: false,
            }]
        );
    }

    #[test]
    fn host_operation_metrics_track_duration_without_tenant_entries() {
        let metrics = RuntimeMetrics::default();

        metrics.record_host_operation_started("document_get");
        metrics.record_host_operation_duration("document_get", Duration::from_millis(3));
        metrics.record_host_operation_succeeded("document_get");

        metrics.record_host_operation_started("ctx_service_lookup");
        metrics.record_host_operation_duration("ctx_service_lookup", Duration::from_millis(5));
        metrics.record_host_operation_failed("ctx_service_lookup");

        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot
                .host_operations
                .get("document_get")
                .expect("document_get host operation should be recorded"),
            &RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 1,
                failed: 0,
                canceled_before_start: 0,
                canceled_in_flight: 0,
                nanos_total: 3_000_000,
            }
        );
        assert_eq!(
            snapshot
                .host_operations
                .get("ctx_service_lookup")
                .expect("ctx_service_lookup host operation should be recorded"),
            &RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 0,
                failed: 1,
                canceled_before_start: 0,
                canceled_in_flight: 0,
                nanos_total: 5_000_000,
            }
        );
        assert!(
            snapshot.tenants.is_empty(),
            "host operation metrics must not create tenant-cardinality labels"
        );
    }

    #[test]
    fn wasmtime_metrics_snapshot_is_low_cardinality_global_state() {
        let metrics = RuntimeMetrics::default();

        metrics.record_wasmtime_module_cache_hit();
        metrics.record_wasmtime_module_cache_miss();
        metrics.record_wasmtime_module_compilation_time(Duration::from_micros(42));
        metrics.record_wasmtime_fuel_consumed(17);
        metrics.record_wasmtime_fuel_exhaustion();
        metrics.record_wasmtime_store_pool_hit();
        metrics.record_wasmtime_store_pool_miss();
        metrics.record_wasmtime_store_pool_authority_mismatch();
        metrics.record_wasmtime_store_pool_eviction();
        metrics.record_wasmtime_store_pool_retirement();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.wasmtime_module_cache_hits, 1);
        assert_eq!(snapshot.wasmtime_module_cache_misses, 1);
        assert_eq!(snapshot.wasmtime_module_compilations, 1);
        assert_eq!(snapshot.wasmtime_module_compilation_nanos_total, 42_000);
        assert_eq!(snapshot.wasmtime_fuel_consumed_total, 17);
        assert_eq!(snapshot.wasmtime_fuel_exhaustions, 1);
        assert_eq!(snapshot.wasmtime_store_pool_hits, 1);
        assert_eq!(snapshot.wasmtime_store_pool_misses, 1);
        assert_eq!(snapshot.wasmtime_store_pool_authority_mismatches, 1);
        assert_eq!(snapshot.wasmtime_store_pool_evictions, 1);
        assert_eq!(snapshot.wasmtime_store_pool_retirements, 1);
        assert!(
            snapshot.tenants.is_empty(),
            "Wasmtime backend metrics must not add tenant-cardinality labels"
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
        metrics.record_request_correlation_duration(Duration::from_millis(16));
        metrics.record_execution_plan_build(Duration::from_millis(17));
        metrics.record_admission_decision(Duration::from_millis(18));
        metrics.record_worker_router_dispatch(Duration::from_millis(19));
        metrics.update_worker_affinity_cache_entries(1);
        metrics.record_worker_affinity_cache_eviction();
        metrics.increment_retained_runtime_pool_entries();
        metrics.record_retained_runtime_pool_eviction();
        metrics.record_retained_runtime_pool_retirement();
        metrics.record_bundle_load(Duration::from_millis(5));
        metrics.record_bundle_integrity_verify(Duration::from_millis(20));
        metrics.record_bundle_module_load(Duration::from_millis(6));
        metrics.record_bundle_evaluation(Duration::from_millis(7));
        metrics.record_fresh_realm_create(Duration::from_millis(8));
        metrics.record_fresh_realm_bootstrap_install(Duration::from_millis(9));
        metrics.record_fresh_realm_bootstrap_finalize(Duration::from_millis(10));
        metrics.record_fresh_realm_bootstrap_reset(Duration::from_millis(11));
        metrics.record_fresh_realm_invocation_script(Duration::from_millis(12));
        metrics.record_fresh_realm_promise_resolve(Duration::from_millis(13));
        metrics.record_fresh_realm_deserialization(Duration::from_millis(14));
        metrics.record_fresh_realm_destroy(Duration::from_millis(15));
        metrics.decrement_retained_runtime_pool_entries();
        metrics.record_runtime_pool_miss();
        metrics.record_runtime_pool_hit();
        metrics.record_runtime_pool_replacement();
        metrics.record_timeout();
        metrics.record_rejected_invocation_for_tenant(None);
        metrics.record_queued_canceled_invocation();
        metrics.record_precanceled_host_op();
        metrics.record_in_flight_canceled_host_op();
        metrics.record_host_bridge_call(Duration::from_millis(21));
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
                request_correlation_records: 1,
                request_correlation_nanos_total: 16_000_000,
                execution_plan_builds: 1,
                execution_plan_build_nanos_total: 17_000_000,
                admission_decisions: 1,
                admission_decision_nanos_total: 18_000_000,
                worker_router_dispatches: 1,
                worker_router_dispatch_nanos_total: 19_000_000,
                worker_affinity_cache_entries: 1,
                worker_affinity_cache_evictions: 1,
                retained_runtime_pool_entries: 0,
                retained_runtime_pool_evictions: 1,
                retained_runtime_pool_retirements: 1,
                bundle_loads: 1,
                bundle_load_nanos_total: 5_000_000,
                bundle_integrity_verifications: 1,
                bundle_integrity_verify_nanos_total: 20_000_000,
                bundle_module_loads: 1,
                bundle_module_load_nanos_total: 6_000_000,
                bundle_evaluations: 1,
                bundle_evaluation_nanos_total: 7_000_000,
                fresh_realm_creates: 1,
                fresh_realm_create_nanos_total: 8_000_000,
                fresh_realm_bootstrap_installs: 1,
                fresh_realm_bootstrap_install_nanos_total: 9_000_000,
                fresh_realm_bootstrap_finalizes: 1,
                fresh_realm_bootstrap_finalize_nanos_total: 10_000_000,
                fresh_realm_bootstrap_resets: 1,
                fresh_realm_bootstrap_reset_nanos_total: 11_000_000,
                fresh_realm_invocation_scripts: 1,
                fresh_realm_invocation_script_nanos_total: 12_000_000,
                fresh_realm_promise_resolves: 1,
                fresh_realm_promise_resolve_nanos_total: 13_000_000,
                fresh_realm_deserializations: 1,
                fresh_realm_deserialization_nanos_total: 14_000_000,
                fresh_realm_destroys: 1,
                fresh_realm_destroy_nanos_total: 15_000_000,
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
                host_bridge_calls: 1,
                host_bridge_call_nanos_total: 21_000_000,
                nested_local_dispatches: 1,
                fallback_cross_runtime_dispatches: 1,
                warm_pool_hits: 0,
                warm_pool_misses: 0,
                warm_pool_retirements: 0,
                warm_pool_discard_unquiesced: 0,
                wasmtime_module_cache_hits: 0,
                wasmtime_module_cache_misses: 0,
                wasmtime_module_compilations: 0,
                wasmtime_module_compilation_nanos_total: 0,
                wasmtime_fuel_consumed_total: 0,
                wasmtime_fuel_exhaustions: 0,
                wasmtime_store_pool_hits: 0,
                wasmtime_store_pool_misses: 0,
                wasmtime_store_pool_authority_mismatches: 0,
                wasmtime_store_pool_evictions: 0,
                wasmtime_store_pool_retirements: 0,
                host_pressure: RuntimeHostPressureMetricsSnapshot::default(),
                adaptive_controller: RuntimeAdaptiveControllerMetricsSnapshot::default(),
                profiles: RuntimeProfileTelemetrySnapshot::default(),
                host_operations: BTreeMap::new(),
                tenants: BTreeMap::new(),
                recent_request_correlations: Vec::new(),
            }
        );
    }

    fn host_resource_decision(
        host_pressure_level: RuntimeHostPressureLevel,
        cpu_pressure_level: RuntimeHostPressureLevel,
        cpu_source_status: RuntimeHostPressureSourceStatus,
        memory_pressure_level: RuntimeMemoryPressureLevel,
        memory_source_status: RuntimeMemoryPressureSourceStatus,
        nominal_dispatch_seats: usize,
        effective_dispatch_seats: usize,
    ) -> RuntimeHostResourceDecision {
        RuntimeHostResourceDecision {
            host_pressure_level,
            cpu_pressure_level,
            cpu_source_status,
            memory_pressure_level,
            memory_source_status,
            control_plane_lag_high: false,
            runtime_allocatable_millicpus: 4000,
            nominal_dispatch_seats,
            effective_dispatch_seats,
            pause_prewarming: !matches!(host_pressure_level, RuntimeHostPressureLevel::Nominal),
            run_idle_low_memory_maintenance: !matches!(
                memory_pressure_level,
                RuntimeMemoryPressureLevel::Nominal
            ),
            evict_idle_retained_runtimes: !matches!(
                memory_pressure_level,
                RuntimeMemoryPressureLevel::Nominal
            ),
        }
    }

    fn nonzero_usize(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test config constant should be nonzero")
    }
}
