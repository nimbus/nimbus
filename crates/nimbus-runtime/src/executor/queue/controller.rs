use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::Result;

use super::job::RuntimeWorkerJob;
use super::router::RuntimeWorkerRouter;
use super::signal::WorkerActivitySignal;

pub(crate) trait RuntimeWorkerQueue: Send + Sync + 'static {
    fn activity_signal(&self) -> Arc<WorkerActivitySignal>;

    fn try_recv(&self) -> Option<RuntimeWorkerJob>;

    fn recv_blocking(&self) -> Option<RuntimeWorkerJob>;

    fn complete_job(
        &self,
        job: RuntimeWorkerJob,
        result: Result<Value>,
        ready_jobs: Vec<RuntimeWorkerJob>,
    );
}

pub(crate) struct RuntimeWorkerQueueController {
    worker_id: usize,
    activity_signal: Arc<WorkerActivitySignal>,
    receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
    router: Arc<RuntimeWorkerRouter>,
}

impl RuntimeWorkerQueueController {
    fn fail_ready_job(ready_job: RuntimeWorkerJob) {
        ready_job
            .result_tx
            .send(Err(RuntimeWorkerRouter::closed_error()));
    }

    pub(super) fn new(
        worker_id: usize,
        receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
        activity_signal: Arc<WorkerActivitySignal>,
        router: Arc<RuntimeWorkerRouter>,
    ) -> Self {
        Self {
            worker_id,
            activity_signal,
            receiver,
            router,
        }
    }
}

impl RuntimeWorkerQueue for RuntimeWorkerQueueController {
    fn activity_signal(&self) -> Arc<WorkerActivitySignal> {
        self.activity_signal.clone()
    }

    fn try_recv(&self) -> Option<RuntimeWorkerJob> {
        let mut receiver = self
            .receiver
            .lock()
            .expect("runtime executor receiver lock should not be poisoned");
        receiver.try_recv().ok()
    }

    fn recv_blocking(&self) -> Option<RuntimeWorkerJob> {
        let mut receiver = self
            .receiver
            .lock()
            .expect("runtime executor receiver lock should not be poisoned");
        receiver.blocking_recv()
    }

    fn complete_job(
        &self,
        job: RuntimeWorkerJob,
        result: Result<Value>,
        ready_jobs: Vec<RuntimeWorkerJob>,
    ) {
        self.router.complete_worker_job(self.worker_id);
        job.result_tx.send(result);
        for ready_job in ready_jobs {
            if let Err(ready_job) = self.router.dispatch_job_blocking(ready_job) {
                Self::fail_ready_job(*ready_job);
            }
        }
    }
}
