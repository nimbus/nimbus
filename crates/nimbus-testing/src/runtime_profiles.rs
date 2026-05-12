use nimbus_runtime::{RuntimeExecutionModel, RuntimeLimits, RuntimePoolKind};

pub fn product_default_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits::default()
}

pub fn run_to_completion_snapshot_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }
}

pub fn cooperative_startup_snapshot_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }
}

pub fn cooperative_warm_pool_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        ..RuntimeLimits::default()
    }
}

pub fn bounded_fairness_runtime_test_limits() -> RuntimeLimits {
    RuntimeLimits {
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        max_concurrent_runtime_instances: 2,
        worker_threads: 2,
        max_active_top_level_invocations_per_tenant: 1,
        max_in_flight_top_level_invocations_per_tenant: 1,
        max_queued_top_level_invocations_per_tenant: 1,
        ..RuntimeLimits::default()
    }
}
