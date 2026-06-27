use std::sync::Arc;

use crate::limits::{RuntimeExecutionModel, RuntimePolicy};
use crate::watchdog::WatchdogTimer;

pub(crate) mod cooperative;
mod run_to_completion;

pub(crate) use cooperative::{CooperativeWorkerLoopFactory, WasmtimeFuelWorkerLoopFactory};
pub(crate) use run_to_completion::{
    RunToCompletionWorkerLoopFactory, WorkerLoop, WorkerLoopFactory,
};

pub(crate) fn create_worker_loop_factory(
    policy: Arc<RuntimePolicy>,
    watchdog: WatchdogTimer,
    #[cfg(test)] test_state: Arc<crate::executor::RuntimeExecutorTestState>,
) -> Arc<dyn WorkerLoopFactory> {
    match policy.limits().execution_model {
        RuntimeExecutionModel::RunToCompletion | RuntimeExecutionModel::BackendOwnedEventLoop => {
            let factory = RunToCompletionWorkerLoopFactory::new(watchdog);
            #[cfg(test)]
            let factory = factory.with_test_state(test_state);
            Arc::new(factory)
        }
        RuntimeExecutionModel::CooperativeFuel => {
            let factory = WasmtimeFuelWorkerLoopFactory::new(watchdog);
            #[cfg(test)]
            let factory = factory.with_test_state(test_state);
            Arc::new(factory)
        }
        RuntimeExecutionModel::CooperativeLocker => {
            let factory = CooperativeWorkerLoopFactory::new(watchdog);
            #[cfg(test)]
            let factory = factory.with_test_state(test_state);
            Arc::new(factory)
        }
    }
}
