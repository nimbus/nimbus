use crate::limits::RuntimeLimits;
#[cfg(test)]
use crate::limits::{
    RuntimeMemoryPressureDecision, RuntimeMemoryPressureLevel, RuntimeMemoryPressureSourceStatus,
};

use super::{
    ReusableV8Runtime,
    embedder::{JsRuntime, v8},
};

const HEAP_CARRYOVER_LIMIT_NUMERATOR: usize = 3;
const HEAP_CARRYOVER_LIMIT_DENOMINATOR: usize = 4;
const MIB_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeHeapUsage {
    pub(crate) total_heap_size_bytes: usize,
    pub(crate) used_heap_size_bytes: usize,
    pub(crate) external_memory_bytes: usize,
    pub(crate) heap_size_limit_bytes: usize,
    pub(crate) native_context_count: usize,
    pub(crate) detached_context_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WarmRuntimeCleanlinessReport {
    pub(crate) warm_reuse_safe_before_reset: bool,
    pub(crate) request_state_reset_succeeded: bool,
    pub(crate) heap_after_maintenance: RuntimeHeapUsage,
    pub(crate) retained_memory_bytes: usize,
    pub(crate) carryover_limit_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WarmRuntimeBoundaryMaintenance {
    pub(crate) heap_before: RuntimeHeapUsage,
    pub(crate) heap_after: RuntimeHeapUsage,
    pub(crate) moderate_memory_pressure_notifications: usize,
    pub(crate) low_memory_notifications: usize,
    pub(crate) cleanliness: WarmRuntimeCleanlinessReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WarmRuntimeCondemnationReason {
    MaxWarmReusesExceeded {
        reuse_count: usize,
        max_warm_reuses: usize,
    },
    HeapCarryoverExceeded {
        report: WarmRuntimeCleanlinessReport,
        retained_memory_bytes: usize,
        used_heap_size_bytes: usize,
        external_memory_bytes: usize,
        carryover_limit_bytes: usize,
    },
    EventLoopNotQuiescent {
        report: WarmRuntimeCleanlinessReport,
    },
    RequestStateResetFailed {
        report: WarmRuntimeCleanlinessReport,
    },
    DetachedContextsPresent {
        report: WarmRuntimeCleanlinessReport,
        detached_context_count: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WarmRuntimeRetentionDecision {
    Retain(WarmRuntimeBoundaryMaintenance),
    Condemn(WarmRuntimeCondemnationReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeReuseLifecycleState {
    Cold,
    Bootstrapping,
    Ready,
    Leased,
    Draining,
    CleanReturn,
    DirtyDiscard,
    Condemned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeReuseLifecycle {
    state: RuntimeReuseLifecycleState,
    #[cfg(test)]
    history: Vec<RuntimeReuseLifecycleState>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WarmPoolMemoryPressureEviction {
    pub(crate) pressure: RuntimeMemoryPressureLevel,
    pub(crate) evicted_entries: usize,
    pub(crate) retained_entries: usize,
}

impl RuntimeReuseLifecycle {
    pub(crate) fn bootstrapped_and_leased() -> Self {
        let mut lifecycle = Self::new(RuntimeReuseLifecycleState::Cold);
        lifecycle.transition(RuntimeReuseLifecycleState::Bootstrapping);
        lifecycle.transition(RuntimeReuseLifecycleState::Ready);
        lifecycle.transition(RuntimeReuseLifecycleState::Leased);
        lifecycle
    }

    pub(crate) fn mark_leased(&mut self) {
        self.transition(RuntimeReuseLifecycleState::Leased);
    }

    pub(crate) fn mark_draining(&mut self) {
        self.transition(RuntimeReuseLifecycleState::Draining);
    }

    pub(crate) fn mark_clean_return(&mut self) {
        self.transition(RuntimeReuseLifecycleState::CleanReturn);
        self.transition(RuntimeReuseLifecycleState::Ready);
    }

    pub(crate) fn mark_dirty_discard(&mut self) {
        self.transition(RuntimeReuseLifecycleState::DirtyDiscard);
    }

    pub(crate) fn mark_condemned(&mut self) {
        self.transition(RuntimeReuseLifecycleState::Condemned);
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> RuntimeReuseLifecycleState {
        self.state
    }

    #[cfg(test)]
    pub(crate) fn history(&self) -> &[RuntimeReuseLifecycleState] {
        &self.history
    }

    fn new(state: RuntimeReuseLifecycleState) -> Self {
        Self {
            state,
            #[cfg(test)]
            history: vec![state],
        }
    }

    fn transition(&mut self, state: RuntimeReuseLifecycleState) {
        self.state = state;
        #[cfg(test)]
        self.history.push(state);
    }
}

pub(crate) fn prepare_warm_runtime_for_retention(
    runtime: &mut ReusableV8Runtime,
    limits: &RuntimeLimits,
) -> WarmRuntimeRetentionDecision {
    if runtime.warm_reuse_count >= limits.max_warm_reuses {
        release_lock_if_held(&mut runtime.runtime);
        return WarmRuntimeRetentionDecision::Condemn(
            WarmRuntimeCondemnationReason::MaxWarmReusesExceeded {
                reuse_count: runtime.warm_reuse_count,
                max_warm_reuses: limits.max_warm_reuses,
            },
        );
    }

    if !runtime.runtime.is_warm_reuse_safe() {
        let report =
            WarmRuntimeCleanlinessReport::current(&mut runtime.runtime, limits, false, false);
        release_lock_if_held(&mut runtime.runtime);
        return WarmRuntimeRetentionDecision::Condemn(
            WarmRuntimeCondemnationReason::EventLoopNotQuiescent { report },
        );
    }

    if runtime.runtime.reset_request_state().is_err() {
        let report =
            WarmRuntimeCleanlinessReport::current(&mut runtime.runtime, limits, true, false);
        release_lock_if_held(&mut runtime.runtime);
        return WarmRuntimeRetentionDecision::Condemn(
            WarmRuntimeCondemnationReason::RequestStateResetFailed { report },
        );
    }

    let carryover_limit_bytes = heap_carryover_limit_bytes(limits);
    let maintenance = run_boundary_memory_maintenance(&mut runtime.runtime, carryover_limit_bytes);
    let carryover_limit_bytes = maintenance.cleanliness.carryover_limit_bytes;
    let retained_memory_bytes = maintenance.heap_after.retained_memory_bytes();
    if maintenance.heap_after.detached_context_count != 0 {
        return WarmRuntimeRetentionDecision::Condemn(
            WarmRuntimeCondemnationReason::DetachedContextsPresent {
                report: maintenance.cleanliness,
                detached_context_count: maintenance.heap_after.detached_context_count,
            },
        );
    }

    if retained_memory_bytes > carryover_limit_bytes {
        return WarmRuntimeRetentionDecision::Condemn(
            WarmRuntimeCondemnationReason::HeapCarryoverExceeded {
                report: maintenance.cleanliness,
                retained_memory_bytes,
                used_heap_size_bytes: maintenance.heap_after.used_heap_size_bytes,
                external_memory_bytes: maintenance.heap_after.external_memory_bytes,
                carryover_limit_bytes,
            },
        );
    }

    WarmRuntimeRetentionDecision::Retain(maintenance)
}

#[cfg(test)]
pub(crate) fn retained_entry_eviction_count_for_pressure(
    pressure: RuntimeMemoryPressureLevel,
    retained_entries: usize,
) -> usize {
    RuntimeMemoryPressureDecision::for_level(pressure, RuntimeMemoryPressureSourceStatus::Observed)
        .retained_runtime_eviction_target(retained_entries)
}

fn run_boundary_memory_maintenance(
    runtime: &mut JsRuntime,
    carryover_limit_bytes: usize,
) -> WarmRuntimeBoundaryMaintenance {
    let (heap_before, heap_after, cleanliness) = {
        let isolate = runtime.v8_isolate();
        let heap_before = RuntimeHeapUsage::from_statistics(isolate.get_heap_statistics());
        isolate.memory_pressure_notification(v8::MemoryPressureLevel::Moderate);
        isolate.low_memory_notification();
        let heap_after = RuntimeHeapUsage::from_statistics(isolate.get_heap_statistics());
        let cleanliness = WarmRuntimeCleanlinessReport::from_heap_after_maintenance(
            heap_after,
            true,
            true,
            carryover_limit_bytes,
        );
        (heap_before, heap_after, cleanliness)
    };
    release_lock_if_held(runtime);

    WarmRuntimeBoundaryMaintenance {
        heap_before,
        heap_after,
        moderate_memory_pressure_notifications: 1,
        low_memory_notifications: 1,
        cleanliness,
    }
}

fn release_lock_if_held(runtime: &mut JsRuntime) {
    if runtime.is_v8_lock_held() {
        runtime.release_v8_lock();
    }
}

fn heap_carryover_limit_bytes(limits: &RuntimeLimits) -> usize {
    limits
        .max_heap_mb
        .saturating_mul(MIB_BYTES)
        .saturating_mul(HEAP_CARRYOVER_LIMIT_NUMERATOR)
        / HEAP_CARRYOVER_LIMIT_DENOMINATOR
}

impl WarmRuntimeCleanlinessReport {
    fn current(
        runtime: &mut JsRuntime,
        limits: &RuntimeLimits,
        warm_reuse_safe_before_reset: bool,
        request_state_reset_succeeded: bool,
    ) -> Self {
        let heap_after_maintenance =
            RuntimeHeapUsage::from_statistics(runtime.v8_isolate().get_heap_statistics());
        Self::from_heap_after_maintenance(
            heap_after_maintenance,
            warm_reuse_safe_before_reset,
            request_state_reset_succeeded,
            heap_carryover_limit_bytes(limits),
        )
    }

    fn from_heap_after_maintenance(
        heap_after_maintenance: RuntimeHeapUsage,
        warm_reuse_safe_before_reset: bool,
        request_state_reset_succeeded: bool,
        carryover_limit_bytes: usize,
    ) -> Self {
        Self {
            warm_reuse_safe_before_reset,
            request_state_reset_succeeded,
            heap_after_maintenance,
            retained_memory_bytes: heap_after_maintenance.retained_memory_bytes(),
            carryover_limit_bytes,
        }
    }
}

impl RuntimeHeapUsage {
    pub(crate) fn retained_memory_bytes(self) -> usize {
        self.used_heap_size_bytes
            .saturating_add(self.external_memory_bytes)
    }

    fn from_statistics(statistics: v8::HeapStatistics) -> Self {
        Self {
            total_heap_size_bytes: statistics.total_heap_size(),
            used_heap_size_bytes: statistics.used_heap_size(),
            external_memory_bytes: statistics.external_memory(),
            heap_size_limit_bytes: statistics.heap_size_limit(),
            native_context_count: statistics.number_of_native_contexts(),
            detached_context_count: statistics.number_of_detached_contexts(),
        }
    }
}

#[cfg(test)]
pub(crate) fn heap_carryover_limit_bytes_for_test(limits: &RuntimeLimits) -> usize {
    heap_carryover_limit_bytes(limits)
}

#[cfg(test)]
pub(crate) fn retained_entry_eviction_count_for_pressure_for_test(
    pressure: RuntimeMemoryPressureLevel,
    retained_entries: usize,
) -> usize {
    retained_entry_eviction_count_for_pressure(pressure, retained_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_reuse_lifecycle_tracks_clean_return_to_ready() {
        let mut lifecycle = RuntimeReuseLifecycle::bootstrapped_and_leased();
        assert_eq!(
            lifecycle.history(),
            &[
                RuntimeReuseLifecycleState::Cold,
                RuntimeReuseLifecycleState::Bootstrapping,
                RuntimeReuseLifecycleState::Ready,
                RuntimeReuseLifecycleState::Leased,
            ]
        );

        lifecycle.mark_draining();
        lifecycle.mark_clean_return();

        assert_eq!(lifecycle.state(), RuntimeReuseLifecycleState::Ready);
        assert_eq!(
            lifecycle.history(),
            &[
                RuntimeReuseLifecycleState::Cold,
                RuntimeReuseLifecycleState::Bootstrapping,
                RuntimeReuseLifecycleState::Ready,
                RuntimeReuseLifecycleState::Leased,
                RuntimeReuseLifecycleState::Draining,
                RuntimeReuseLifecycleState::CleanReturn,
                RuntimeReuseLifecycleState::Ready,
            ]
        );
    }

    #[test]
    fn runtime_reuse_lifecycle_records_terminal_discard_states() {
        let mut dirty = RuntimeReuseLifecycle::bootstrapped_and_leased();
        dirty.mark_draining();
        dirty.mark_dirty_discard();
        assert_eq!(dirty.state(), RuntimeReuseLifecycleState::DirtyDiscard);

        let mut condemned = RuntimeReuseLifecycle::bootstrapped_and_leased();
        condemned.mark_draining();
        condemned.mark_condemned();
        assert_eq!(condemned.state(), RuntimeReuseLifecycleState::Condemned);
    }

    #[test]
    fn retained_memory_bytes_includes_v8_reported_external_memory() {
        let usage = RuntimeHeapUsage {
            total_heap_size_bytes: 16 * MIB_BYTES,
            used_heap_size_bytes: 7 * MIB_BYTES,
            external_memory_bytes: 5 * MIB_BYTES,
            heap_size_limit_bytes: 128 * MIB_BYTES,
            native_context_count: 1,
            detached_context_count: 0,
        };

        assert_eq!(usage.retained_memory_bytes(), 12 * MIB_BYTES);
    }

    #[test]
    fn runtime_cleanliness_report_records_context_counts_and_configured_limit() {
        let heap_after_maintenance = RuntimeHeapUsage {
            total_heap_size_bytes: 16 * MIB_BYTES,
            used_heap_size_bytes: 7 * MIB_BYTES,
            external_memory_bytes: 5 * MIB_BYTES,
            heap_size_limit_bytes: 128 * MIB_BYTES,
            native_context_count: 3,
            detached_context_count: 2,
        };

        let report = WarmRuntimeCleanlinessReport::from_heap_after_maintenance(
            heap_after_maintenance,
            true,
            true,
            48 * MIB_BYTES,
        );

        assert!(report.warm_reuse_safe_before_reset);
        assert!(report.request_state_reset_succeeded);
        assert_eq!(report.heap_after_maintenance.native_context_count, 3);
        assert_eq!(report.heap_after_maintenance.detached_context_count, 2);
        assert_eq!(report.retained_memory_bytes, 12 * MIB_BYTES);
        assert_eq!(report.carryover_limit_bytes, 48 * MIB_BYTES);
    }
}
