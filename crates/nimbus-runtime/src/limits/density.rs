use std::num::NonZeroUsize;

use serde::Serialize;

use super::{RuntimeCompatibilityTarget, RuntimeLimits, RuntimeProfile};

const MIB_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDensityMeasurementMethod {
    ProcessRssDelta,
    ChildProcessRssDelta,
    ExternalProfiler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeDensityMeasurement {
    pub profile: RuntimeProfile,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub method: RuntimeDensityMeasurementMethod,
    pub retained_runtime_count: NonZeroUsize,
    pub total_rss_delta_bytes: u64,
}

impl RuntimeDensityMeasurement {
    pub fn from_total_rss_delta(
        profile: RuntimeProfile,
        compatibility_target: RuntimeCompatibilityTarget,
        method: RuntimeDensityMeasurementMethod,
        retained_runtime_count: NonZeroUsize,
        total_rss_delta_bytes: u64,
    ) -> Self {
        assert_eq!(
            RuntimeProfile::for_compatibility_target(compatibility_target),
            Some(profile),
            "density measurements must match the compatibility target profile"
        );
        Self {
            profile,
            compatibility_target,
            method,
            retained_runtime_count,
            total_rss_delta_bytes,
        }
    }

    pub fn per_runtime_rss_bytes_ceil(self) -> u64 {
        div_ceil_u64(
            self.total_rss_delta_bytes.max(1),
            usize_to_u64_saturating(self.retained_runtime_count.get()).max(1),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeDensityBudget {
    pub host_runtime_budget_bytes: u64,
    pub operator_reserved_headroom_bytes: u64,
}

impl RuntimeDensityBudget {
    pub fn available_runtime_budget_bytes(self) -> u64 {
        self.host_runtime_budget_bytes
            .saturating_sub(self.operator_reserved_headroom_bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeIsolateGroupFfiStatus {
    DeferredPendingValidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeDensityPlan {
    pub profile: RuntimeProfile,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub measured_per_runtime_rss_bytes: u64,
    pub heap_cap_bytes_per_runtime: u64,
    pub planning_bytes_per_runtime: u64,
    pub host_runtime_budget_bytes: u64,
    pub operator_reserved_headroom_bytes: u64,
    pub available_runtime_budget_bytes: u64,
    pub active_runtime_slots_reserved: usize,
    pub active_runtime_reservation_bytes: u64,
    pub retained_pool_budget_bytes: u64,
    pub worker_threads: usize,
    pub configured_max_warm_pool_entries_per_worker: usize,
    pub max_retained_runtimes_by_memory: usize,
    pub max_retained_runtimes_per_worker_by_memory: usize,
    pub effective_max_warm_pool_entries_per_worker: usize,
    pub isolate_group_ffi_status: RuntimeIsolateGroupFfiStatus,
}

impl RuntimeDensityPlan {
    pub fn for_limits_measurement_and_budget(
        limits: &RuntimeLimits,
        measurement: RuntimeDensityMeasurement,
        budget: RuntimeDensityBudget,
    ) -> Self {
        let normalized = limits.normalized();
        assert_eq!(
            RuntimeProfile::for_limits(&normalized),
            Some(measurement.profile),
            "density plan must use a measurement for the normalized runtime profile"
        );
        assert_eq!(
            normalized.compatibility_target, measurement.compatibility_target,
            "density plan must use a measurement for the normalized compatibility target"
        );

        let measured_per_runtime_rss_bytes = measurement.per_runtime_rss_bytes_ceil();
        let heap_cap_bytes_per_runtime = mib_bytes_from_usize(normalized.max_heap_mb);
        let planning_bytes_per_runtime =
            measured_per_runtime_rss_bytes.max(heap_cap_bytes_per_runtime);
        let available_runtime_budget_bytes = budget.available_runtime_budget_bytes();
        let active_runtime_slots_reserved = normalized.max_concurrent_runtime_instances;
        let active_runtime_reservation_bytes = planning_bytes_per_runtime
            .saturating_mul(usize_to_u64_saturating(active_runtime_slots_reserved));
        let retained_pool_budget_bytes =
            available_runtime_budget_bytes.saturating_sub(active_runtime_reservation_bytes);
        let max_retained_runtimes_by_memory = usize_from_u64_saturating(
            retained_pool_budget_bytes / planning_bytes_per_runtime.max(1),
        );
        let worker_threads = normalized.worker_threads.max(1);
        let max_retained_runtimes_per_worker_by_memory =
            max_retained_runtimes_by_memory / worker_threads;
        let configured_max_warm_pool_entries_per_worker =
            normalized.max_warm_pool_entries_per_worker;
        let effective_max_warm_pool_entries_per_worker =
            configured_max_warm_pool_entries_per_worker
                .min(max_retained_runtimes_per_worker_by_memory);

        Self {
            profile: measurement.profile,
            compatibility_target: measurement.compatibility_target,
            measured_per_runtime_rss_bytes,
            heap_cap_bytes_per_runtime,
            planning_bytes_per_runtime,
            host_runtime_budget_bytes: budget.host_runtime_budget_bytes,
            operator_reserved_headroom_bytes: budget.operator_reserved_headroom_bytes,
            available_runtime_budget_bytes,
            active_runtime_slots_reserved,
            active_runtime_reservation_bytes,
            retained_pool_budget_bytes,
            worker_threads,
            configured_max_warm_pool_entries_per_worker,
            max_retained_runtimes_by_memory,
            max_retained_runtimes_per_worker_by_memory,
            effective_max_warm_pool_entries_per_worker,
            isolate_group_ffi_status: RuntimeIsolateGroupFfiStatus::DeferredPendingValidation,
        }
    }

    pub fn isolate_group_ffi_allowed(self) -> bool {
        !matches!(
            self.isolate_group_ffi_status,
            RuntimeIsolateGroupFfiStatus::DeferredPendingValidation
        )
    }
}

fn div_ceil_u64(numerator: u64, denominator: u64) -> u64 {
    numerator / denominator + u64::from(numerator % denominator != 0)
}

fn mib_bytes_from_usize(mebibytes: usize) -> u64 {
    usize_to_u64_saturating(mebibytes).saturating_mul(MIB_BYTES)
}

fn usize_to_u64_saturating(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn usize_from_u64_saturating(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
