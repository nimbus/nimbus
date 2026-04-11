use std::sync::Arc;
use std::time::Instant;

use crate::backends::v8::{DeferredV8RuntimeDropQueue, V8WorkerRuntimePool};
use crate::executor::{RuntimeWorkerJob, SharedInvocationPermit};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::CooperativeLockerRuntimeSlot;
use crate::watchdog::WatchdogTimer;

use super::{WorkerLoop, WorkerLoopFactory};

mod execution;
mod retention;
mod run;
mod scheduler;

use self::scheduler::{CooperativeRunnableSlot, CooperativeScheduler};

pub(crate) struct CooperativeWorkerLoopFactory {
    watchdog: WatchdogTimer,
    #[cfg(test)]
    test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
}

impl CooperativeWorkerLoopFactory {
    pub(crate) fn new(watchdog: WatchdogTimer) -> Self {
        Self {
            watchdog,
            #[cfg(test)]
            test_state: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_state(
        mut self,
        test_state: Arc<crate::executor::RuntimeExecutorTestState>,
    ) -> Self {
        self.test_state = Some(test_state);
        self
    }
}

impl WorkerLoopFactory for CooperativeWorkerLoopFactory {
    fn create(&self, worker_id: usize, policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop> {
        Box::new(CooperativeWorkerLoop::new(
            worker_id,
            policy,
            self.watchdog.clone(),
            #[cfg(test)]
            self.test_state.clone(),
        ))
    }
}

struct CooperativeWorkerLoop {
    worker_id: usize,
    policy: Arc<RuntimePolicy>,
    watchdog: WatchdogTimer,
    worker_runtime: tokio::runtime::Runtime,
    v8_runtime_pool: V8WorkerRuntimePool,
    activity_signal: Arc<crate::executor::WorkerActivitySignal>,
    activity_generation: u64,
    scheduler: CooperativeScheduler<CooperativeInvocation>,
    deferred_v8_runtime_drops: DeferredV8RuntimeDropQueue,
}

struct CooperativeInvocation {
    job: RuntimeWorkerJob,
    permit: SharedInvocationPermit,
    slot: CooperativeLockerRuntimeSlot,
    execution_started_at: Instant,
    cancellation_for_metrics: Option<HostCallCancellation>,
}

impl CooperativeWorkerLoop {
    fn new(
        worker_id: usize,
        policy: Arc<RuntimePolicy>,
        watchdog: WatchdogTimer,
        #[cfg(test)] test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
    ) -> Self {
        let worker_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|error| {
                panic!("cooperative runtime worker failed to build tokio runtime: {error}")
            });
        #[cfg(test)]
        if let Some(test_state) = &test_state {
            test_state.register_current_worker_runtime();
        }
        let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
        let activity_generation = activity_signal.current_generation();
        Self {
            worker_id,
            policy,
            watchdog,
            worker_runtime,
            v8_runtime_pool: V8WorkerRuntimePool::new(),
            activity_signal,
            activity_generation,
            scheduler: CooperativeScheduler::new(),
            deferred_v8_runtime_drops: DeferredV8RuntimeDropQueue::new(),
        }
    }
}
