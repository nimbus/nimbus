use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle};

use super::admission::RuntimeInvocationDispatchHandle;

pub(crate) struct RuntimeWorkerJob {
    pub(crate) runtime: NeovexRuntime,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) enqueued_at: Instant,
    pub(crate) result_tx: RuntimeWorkerResultSender,
    pub(crate) dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
}

pub(crate) enum RuntimeWorkerResultSender {
    Async(oneshot::Sender<Result<Value>>),
    Blocking(std::sync::mpsc::SyncSender<Result<Value>>),
}

impl RuntimeWorkerResultSender {
    pub(crate) fn send(self, result: Result<Value>) {
        match self {
            Self::Async(result_tx) => {
                let _ = result_tx.send(result);
            }
            Self::Blocking(result_tx) => {
                let _ = result_tx.send(result);
            }
        }
    }
}

pub(crate) trait RuntimeWorkerQueue: Send + Sync + 'static {
    fn recv_blocking(&self) -> Option<RuntimeWorkerJob>;

    fn complete_job(
        &self,
        job: RuntimeWorkerJob,
        result: Result<Value>,
        ready_jobs: Vec<RuntimeWorkerJob>,
    );
}

#[derive(Clone)]
pub(crate) struct RuntimeWorkerShutdown {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl RuntimeWorkerShutdown {
    pub(super) fn new() -> Self {
        Self {
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub(super) fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub(super) struct RuntimeWorkerQueueController {
    receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
    sender: Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
}

impl RuntimeWorkerQueueController {
    fn closed_error() -> NeovexRuntimeError {
        NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
    }

    fn dispatch_sender(&self) -> Result<mpsc::Sender<RuntimeWorkerJob>> {
        self.sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(Self::closed_error)
    }

    fn fail_ready_job(
        dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
        ready_job: RuntimeWorkerJob,
    ) {
        if let Some(dispatch_handle) = dispatch_handle {
            dispatch_handle.rollback_dispatch();
        }
        ready_job.result_tx.send(Err(Self::closed_error()));
    }

    pub(super) fn new(
        receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
        sender: Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
    ) -> Self {
        Self { receiver, sender }
    }

    pub(super) async fn dispatch_job(&self, job: RuntimeWorkerJob) -> Result<()> {
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self.dispatch_sender()?;
        sender.send(job).await.map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            Self::closed_error()
        })
    }

    pub(super) fn dispatch_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self.dispatch_sender()?;
        sender.blocking_send(job).map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            Self::closed_error()
        })
    }

    pub(super) fn close(&self) {
        self.sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .take();
    }
}

impl RuntimeWorkerQueue for RuntimeWorkerQueueController {
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
        job.result_tx.send(result);
        for ready_job in ready_jobs {
            let dispatch_handle = ready_job.dispatch_handle.clone();
            match self.dispatch_sender() {
                Ok(dispatch_sender) => match dispatch_sender.blocking_send(ready_job) {
                    Ok(()) => {}
                    Err(error) => {
                        Self::fail_ready_job(dispatch_handle, error.0);
                    }
                },
                Err(_) => Self::fail_ready_job(dispatch_handle, ready_job),
            }
        }
    }
}
