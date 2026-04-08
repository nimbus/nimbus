use super::*;

#[test]
#[should_panic(expected = "WarmModulePool requires CooperativeLocker")]
fn warm_module_pool_with_run_to_completion_fails_fast() {
    let _policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: crate::limits::RuntimePoolKind::WarmModulePool,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
}
