use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimeRoutingAffinity;
use crate::metrics::RuntimeMetrics;
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

pub(crate) struct WorkerActivitySignal {
    generation: Mutex<u64>,
    condvar: Condvar,
    async_notify: tokio::sync::Notify,
}

impl WorkerActivitySignal {
    pub(crate) fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            condvar: Condvar::new(),
            async_notify: tokio::sync::Notify::new(),
        }
    }

    pub(crate) fn current_generation(&self) -> u64 {
        *self
            .generation
            .lock()
            .expect("worker activity generation lock should not be poisoned")
    }

    pub(crate) fn notify(&self) {
        let mut generation = self
            .generation
            .lock()
            .expect("worker activity generation lock should not be poisoned");
        *generation = generation.saturating_add(1);
        self.condvar.notify_all();
        self.async_notify.notify_waiters();
    }

    pub(crate) async fn wait_for_change_async(&self, last_seen_generation: &mut u64) {
        loop {
            let current_generation = self.current_generation();
            if current_generation != *last_seen_generation {
                *last_seen_generation = current_generation;
                return;
            }

            let notified = self.async_notify.notified();
            let current_generation = self.current_generation();
            if current_generation != *last_seen_generation {
                *last_seen_generation = current_generation;
                return;
            }
            notified.await;
        }
    }
}

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

struct WorkerDispatchQueue {
    sender: Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>,
    load: AtomicUsize,
    last_assigned_sequence: AtomicU64,
    activity_signal: Arc<WorkerActivitySignal>,
}

#[derive(Clone, Copy)]
enum WorkerRouteStrategy {
    Affinity,
    LeastLoaded,
}

#[derive(Clone, Copy)]
struct RuntimeAffinityAssignment {
    worker_id: usize,
    last_assigned_sequence: u64,
}

#[derive(Clone, Copy)]
struct WorkerRouteSelection {
    worker_id: usize,
    strategy: WorkerRouteStrategy,
}

impl WorkerDispatchQueue {
    fn new(
        sender: mpsc::Sender<RuntimeWorkerJob>,
        activity_signal: Arc<WorkerActivitySignal>,
    ) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
            load: AtomicUsize::new(0),
            last_assigned_sequence: AtomicU64::new(0),
            activity_signal,
        }
    }
}

pub(super) struct RuntimeWorkerRouter {
    workers: Vec<WorkerDispatchQueue>,
    metrics: Arc<RuntimeMetrics>,
    routing_affinity: RuntimeRoutingAffinity,
    routing_affinity_max_entries: usize,
    next_assignment_sequence: AtomicU64,
    affinity: Mutex<HashMap<RuntimeAffinityKey, RuntimeAffinityAssignment>>,
}

impl RuntimeWorkerRouter {
    pub(super) fn new(
        worker_count: usize,
        queue_capacity: usize,
        metrics: Arc<RuntimeMetrics>,
        routing_affinity: RuntimeRoutingAffinity,
        routing_affinity_max_entries: usize,
    ) -> (Arc<Self>, Vec<Arc<RuntimeWorkerQueueController>>) {
        let mut workers = Vec::with_capacity(worker_count);
        let mut worker_receivers = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let (sender, receiver) = mpsc::channel::<RuntimeWorkerJob>(queue_capacity);
            let activity_signal = Arc::new(WorkerActivitySignal::new());
            workers.push(WorkerDispatchQueue::new(sender, activity_signal.clone()));
            worker_receivers.push((Arc::new(Mutex::new(receiver)), activity_signal));
        }

        let router = Arc::new(Self {
            workers,
            metrics,
            routing_affinity,
            routing_affinity_max_entries,
            next_assignment_sequence: AtomicU64::new(1),
            affinity: Mutex::new(HashMap::new()),
        });

        let queues = worker_receivers
            .into_iter()
            .enumerate()
            .map(|(worker_id, (receiver, activity_signal))| {
                Arc::new(RuntimeWorkerQueueController::new(
                    worker_id,
                    receiver,
                    activity_signal,
                    router.clone(),
                ))
            })
            .collect();

        (router, queues)
    }

    pub(super) fn closed_error() -> NeovexRuntimeError {
        NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
    }

    fn dispatch_sender(&self, worker_id: usize) -> Result<mpsc::Sender<RuntimeWorkerJob>> {
        self.workers[worker_id]
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(Self::closed_error)
    }

    fn affinity_key(&self, job: &RuntimeWorkerJob) -> Option<RuntimeAffinityKey> {
        runtime_affinity_key(self.routing_affinity, Some(&job.context), &job.bundle)
    }

    fn choose_worker(&self, affinity_key: Option<&RuntimeAffinityKey>) -> WorkerRouteSelection {
        let least_loaded = self
            .workers
            .iter()
            .enumerate()
            .min_by_key(|(_, worker)| {
                (
                    worker.load.load(Ordering::SeqCst),
                    worker.last_assigned_sequence.load(Ordering::SeqCst),
                )
            })
            .map(|(worker_id, _)| WorkerRouteSelection {
                worker_id,
                strategy: WorkerRouteStrategy::LeastLoaded,
            })
            .unwrap_or(WorkerRouteSelection {
                worker_id: 0,
                strategy: WorkerRouteStrategy::LeastLoaded,
            });

        if let Some(worker_id) = affinity_key.and_then(|affinity_key| {
            self.affinity
                .lock()
                .expect("worker affinity lock should not be poisoned")
                .get(affinity_key)
                .map(|assignment| assignment.worker_id)
        }) {
            let affinity_load = self.workers[worker_id].load.load(Ordering::SeqCst);
            let least_loaded_load = self.workers[least_loaded.worker_id]
                .load
                .load(Ordering::SeqCst);
            if affinity_load <= least_loaded_load {
                return WorkerRouteSelection {
                    worker_id,
                    strategy: WorkerRouteStrategy::Affinity,
                };
            }
        }

        least_loaded
    }

    fn record_route(&self, strategy: WorkerRouteStrategy) {
        match strategy {
            WorkerRouteStrategy::Affinity => self.metrics.record_worker_affinity_route(),
            WorkerRouteStrategy::LeastLoaded => self.metrics.record_worker_least_loaded_route(),
        }
    }

    fn note_assignment(&self, worker_id: usize, affinity_key: Option<RuntimeAffinityKey>) {
        let sequence = self.next_assignment_sequence.fetch_add(1, Ordering::SeqCst);
        let worker = &self.workers[worker_id];
        worker
            .last_assigned_sequence
            .store(sequence, Ordering::SeqCst);
        worker.load.fetch_add(1, Ordering::SeqCst);
        if let Some(affinity_key) = affinity_key {
            let mut affinity = self
                .affinity
                .lock()
                .expect("worker affinity lock should not be poisoned");
            affinity.insert(
                affinity_key,
                RuntimeAffinityAssignment {
                    worker_id,
                    last_assigned_sequence: sequence,
                },
            );
            while affinity.len() > self.routing_affinity_max_entries {
                let Some(evicted_key) = affinity
                    .iter()
                    .min_by_key(|(_, assignment)| assignment.last_assigned_sequence)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                affinity.remove(&evicted_key);
                self.metrics.record_worker_affinity_cache_eviction();
            }
            self.metrics
                .update_worker_affinity_cache_entries(affinity.len());
        }
    }

    pub(super) async fn dispatch_job(&self, job: RuntimeWorkerJob) -> Result<()> {
        let affinity_key = self.affinity_key(&job);
        let selection = self.choose_worker(affinity_key.as_ref());
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self.dispatch_sender(selection.worker_id)?;
        sender.send(job).await.map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            Self::closed_error()
        })?;
        self.record_route(selection.strategy);
        self.note_assignment(selection.worker_id, affinity_key);
        self.workers[selection.worker_id].activity_signal.notify();
        Ok(())
    }

    pub(super) fn dispatch_job_blocking(
        &self,
        job: RuntimeWorkerJob,
    ) -> std::result::Result<(), Box<RuntimeWorkerJob>> {
        let affinity_key = self.affinity_key(&job);
        let selection = self.choose_worker(affinity_key.as_ref());
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = match self.dispatch_sender(selection.worker_id) {
            Ok(sender) => sender,
            Err(_) => {
                if let Some(dispatch_handle) = dispatch_handle {
                    dispatch_handle.rollback_dispatch();
                }
                return Err(Box::new(job));
            }
        };
        sender.blocking_send(job).map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            Box::new(error.0)
        })?;
        self.record_route(selection.strategy);
        self.note_assignment(selection.worker_id, affinity_key);
        self.workers[selection.worker_id].activity_signal.notify();
        Ok(())
    }

    pub(super) fn complete_worker_job(&self, worker_id: usize) {
        let worker = &self.workers[worker_id];
        let update = worker
            .load
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_sub(1)
            });
        debug_assert!(update.is_ok(), "worker load should not underflow");
    }

    pub(super) fn close(&self) {
        for worker in &self.workers {
            worker
                .sender
                .lock()
                .expect("runtime executor sender lock should not be poisoned")
                .take();
            worker.activity_signal.notify();
        }
        let mut affinity = self
            .affinity
            .lock()
            .expect("worker affinity lock should not be poisoned");
        affinity.clear();
        self.metrics.update_worker_affinity_cache_entries(0);
    }
}

pub(super) struct RuntimeWorkerQueueController {
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

    fn new(
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
