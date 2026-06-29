use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use crate::backends::wasmtime::WasmtimeFuelDriver;
use crate::executor::{RuntimeWorkerJob, SharedInvocationPermit};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::watchdog::WatchdogTimer;

use super::{WorkerLoop, WorkerLoopFactory};

pub(crate) mod backend;
mod execution;
mod retention;
mod run;
mod scheduler;

use self::backend::{CooperativeBackendDriver, CooperativeBackendSlot, V8LockerDriver};
use self::scheduler::{CooperativeRunnableSlot, CooperativeScheduler};

pub(crate) struct CooperativeWorkerLoopFactory {
    watchdog: WatchdogTimer,
    #[cfg(test)]
    test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
}

pub(crate) struct WasmtimeFuelWorkerLoopFactory {
    watchdog: WatchdogTimer,
    #[cfg(test)]
    test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
}

impl WasmtimeFuelWorkerLoopFactory {
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
    fn create(&self, worker_id: usize, _policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop> {
        Box::new(CooperativeWorkerLoop::new(
            worker_id,
            self.watchdog.clone(),
            V8LockerDriver::new(),
            #[cfg(test)]
            self.test_state.clone(),
        ))
    }
}

impl WorkerLoopFactory for WasmtimeFuelWorkerLoopFactory {
    fn create(&self, worker_id: usize, _policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop> {
        Box::new(CooperativeWorkerLoop::new(
            worker_id,
            self.watchdog.clone(),
            WasmtimeFuelDriver::new(),
            #[cfg(test)]
            self.test_state.clone(),
        ))
    }
}

struct CooperativeWorkerLoop<D: CooperativeBackendDriver> {
    worker_id: usize,
    watchdog: WatchdogTimer,
    worker_runtime: tokio::runtime::Runtime,
    driver: D,
    activity_signal: Arc<crate::executor::WorkerActivitySignal>,
    activity_generation: u64,
    scheduler: CooperativeScheduler<CooperativeInvocation<D::Slot>>,
    pending_admissions: VecDeque<RuntimeWorkerJob>,
}

struct CooperativeInvocation<S: CooperativeBackendSlot> {
    job: RuntimeWorkerJob,
    permit: SharedInvocationPermit,
    slot: S,
    execution_started_at: Instant,
    cancellation_for_metrics: Option<HostCallCancellation>,
}

impl<D: CooperativeBackendDriver> CooperativeWorkerLoop<D> {
    fn new(
        worker_id: usize,
        watchdog: WatchdogTimer,
        driver: D,
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
            watchdog,
            worker_runtime,
            driver,
            activity_signal,
            activity_generation,
            scheduler: CooperativeScheduler::new(),
            pending_admissions: VecDeque::new(),
        }
    }
}
