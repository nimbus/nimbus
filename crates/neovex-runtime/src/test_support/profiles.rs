use std::sync::Arc;

use crate::limits::{RuntimeExecutionModel, RuntimeLimits, RuntimePolicy, RuntimePoolKind};

pub(crate) fn product_default_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits::default()
}

pub(crate) fn product_default_runtime_test_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(product_default_runtime_test_limits()))
}

pub(crate) fn run_to_completion_snapshot_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }
}

pub(crate) fn run_to_completion_snapshot_runtime_test_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(
        run_to_completion_snapshot_runtime_test_limits(),
    ))
}

pub(crate) fn cooperative_startup_snapshot_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }
}

pub(crate) fn cooperative_startup_snapshot_runtime_test_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(
        cooperative_startup_snapshot_runtime_test_limits(),
    ))
}

pub(crate) fn cooperative_warm_pool_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        ..RuntimeLimits::default()
    }
}

pub(crate) fn cooperative_warm_pool_runtime_test_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(
        cooperative_warm_pool_runtime_test_limits(),
    ))
}

pub(crate) fn bounded_fairness_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        max_concurrent_isolates: 2,
        worker_threads: 2,
        max_active_top_level_invocations_per_tenant: 1,
        max_in_flight_top_level_invocations_per_tenant: 1,
        max_queued_top_level_invocations_per_tenant: 1,
        ..RuntimeLimits::default()
    }
}
