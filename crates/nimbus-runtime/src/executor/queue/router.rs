use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeRoutingAffinity;
use crate::metrics::RuntimeMetrics;

use super::controller::RuntimeWorkerQueueController;
use super::job::RuntimeWorkerJob;
use super::signal::WorkerActivitySignal;

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

struct WorkerAssignment {
    worker_id: usize,
    affinity_key: Option<RuntimeAffinityKey>,
    last_assigned_sequence: u64,
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

pub(in crate::executor) struct RuntimeWorkerRouter {
    workers: Vec<WorkerDispatchQueue>,
    metrics: Arc<RuntimeMetrics>,
    routing_affinity: RuntimeRoutingAffinity,
    routing_affinity_max_entries: usize,
    next_assignment_sequence: AtomicU64,
    affinity: Mutex<HashMap<RuntimeAffinityKey, RuntimeAffinityAssignment>>,
}

impl RuntimeWorkerRouter {
    pub(in crate::executor) fn new(
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

    pub(in crate::executor) fn closed_error() -> NimbusRuntimeError {
        NimbusRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
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

    fn note_assignment(
        &self,
        worker_id: usize,
        affinity_key: Option<RuntimeAffinityKey>,
    ) -> WorkerAssignment {
        let sequence = self.next_assignment_sequence.fetch_add(1, Ordering::SeqCst);
        let worker = &self.workers[worker_id];
        worker
            .last_assigned_sequence
            .store(sequence, Ordering::SeqCst);
        worker.load.fetch_add(1, Ordering::SeqCst);
        if let Some(affinity_key) = affinity_key.as_ref() {
            let mut affinity = self
                .affinity
                .lock()
                .expect("worker affinity lock should not be poisoned");
            affinity.insert(
                affinity_key.clone(),
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
        WorkerAssignment {
            worker_id,
            affinity_key,
            last_assigned_sequence: sequence,
        }
    }

    fn rollback_assignment(&self, assignment: WorkerAssignment) {
        let worker = &self.workers[assignment.worker_id];
        let update = worker
            .load
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_sub(1)
            });
        debug_assert!(update.is_ok(), "worker load rollback should not underflow");
        if let Some(affinity_key) = assignment.affinity_key {
            let mut affinity = self
                .affinity
                .lock()
                .expect("worker affinity lock should not be poisoned");
            let should_remove = affinity.get(&affinity_key).is_some_and(|existing| {
                existing.worker_id == assignment.worker_id
                    && existing.last_assigned_sequence == assignment.last_assigned_sequence
            });
            if should_remove {
                affinity.remove(&affinity_key);
            }
            self.metrics
                .update_worker_affinity_cache_entries(affinity.len());
        }
    }

    pub(in crate::executor) async fn dispatch_job(&self, job: RuntimeWorkerJob) -> Result<()> {
        let affinity_key = self.affinity_key(&job);
        let selection = self.choose_worker(affinity_key.as_ref());
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self.dispatch_sender(selection.worker_id)?;
        // Account for the assignment before enqueueing so a very fast worker
        // completion cannot beat the router's load increment.
        let assignment = self.note_assignment(selection.worker_id, affinity_key);
        sender.send(job).await.map_err(|error| {
            self.rollback_assignment(assignment);
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            Self::closed_error()
        })?;
        self.record_route(selection.strategy);
        self.workers[selection.worker_id].activity_signal.notify();
        Ok(())
    }

    pub(in crate::executor) fn dispatch_job_blocking(
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
        // Account for the assignment before enqueueing so a very fast worker
        // completion cannot beat the router's load increment.
        let assignment = self.note_assignment(selection.worker_id, affinity_key);
        sender.blocking_send(job).map_err(|error| {
            self.rollback_assignment(assignment);
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            Box::new(error.0)
        })?;
        self.record_route(selection.strategy);
        self.workers[selection.worker_id].activity_signal.notify();
        Ok(())
    }

    pub(in crate::executor) fn complete_worker_job(&self, worker_id: usize) {
        let worker = &self.workers[worker_id];
        let update = worker
            .load
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_sub(1)
            });
        debug_assert!(update.is_ok(), "worker load should not underflow");
    }

    pub(in crate::executor) fn close(&self) {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::context::RuntimeInvocationContext;
    use crate::executor::queue::RuntimeWorkerResultSender;
    use crate::host::{HostBridge, HostCallRequest};
    use crate::metrics::RuntimeMetrics;
    use crate::runtime::{InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle};

    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> crate::error::Result<serde_json::Value> {
            Ok(serde_json::Value::Null)
        }
    }

    fn sample_job() -> RuntimeWorkerJob {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_owned(),
            args: serde_json::Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        RuntimeWorkerJob {
            runtime: NimbusRuntime::new(Arc::new(NoopHost)),
            bundle: RuntimeBundle::new(&bundle_path),
            request: request.clone(),
            context: RuntimeInvocationContext::top_level(&request),
            cancellation: None,
            enqueued_at: std::time::Instant::now(),
            result_tx: RuntimeWorkerResultSender::Blocking(std::sync::mpsc::sync_channel(1).0),
            dispatch_handle: None,
        }
    }

    #[test]
    fn failed_dispatch_rolls_back_pre_send_worker_load() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (router, queues) =
            RuntimeWorkerRouter::new(1, 1, metrics, RuntimeRoutingAffinity::None, 1);
        drop(queues);

        let result = router.dispatch_job_blocking(sample_job());
        assert!(
            result.is_err(),
            "closed worker queue should reject dispatch"
        );
        assert_eq!(
            router.workers[0].load.load(Ordering::SeqCst),
            0,
            "failed dispatch should roll back the pre-send worker assignment",
        );
    }
}
