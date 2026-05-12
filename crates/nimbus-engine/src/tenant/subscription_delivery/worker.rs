use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::subscriptions::{dispatch_subscription_work, merge_queued_subscription_work};

#[cfg(test)]
use super::pause::SubscriptionDeliveryPauseState;
use super::queue::SubscriptionDeliveryQueueState;
use super::stats::SubscriptionDeliveryMetrics;
use crate::tenant::TenantRuntime;

pub(super) struct SubscriptionDeliveryWorker {
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
    worker_start_count: AtomicU64,
}

impl SubscriptionDeliveryWorker {
    pub(super) fn new() -> Self {
        Self {
            worker: Mutex::new(None),
            shutdown: Arc::new(AtomicBool::new(false)),
            worker_start_count: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    pub(super) fn start(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<SubscriptionDeliveryQueueState>,
        metrics: Arc<SubscriptionDeliveryMetrics>,
        pause: Arc<SubscriptionDeliveryPauseState>,
    ) {
        self.start_inner(runtime, queue, metrics, Some(pause));
    }

    #[cfg(not(test))]
    pub(super) fn start(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<SubscriptionDeliveryQueueState>,
        metrics: Arc<SubscriptionDeliveryMetrics>,
    ) {
        self.start_inner(runtime, queue, metrics);
    }

    #[cfg(test)]
    fn start_inner(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<SubscriptionDeliveryQueueState>,
        metrics: Arc<SubscriptionDeliveryMetrics>,
        pause: Option<Arc<SubscriptionDeliveryPauseState>>,
    ) {
        let mut worker = self
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.shutdown.store(false, Ordering::Release);
        self.worker_start_count.fetch_add(1, Ordering::Relaxed);
        let runtime = Arc::downgrade(runtime);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("nimbus-subscription-delivery".to_string())
                .spawn(move || run_delivery_worker(runtime, queue, metrics, shutdown, pause))
                .expect("subscription delivery worker should spawn"),
        );
    }

    #[cfg(not(test))]
    fn start_inner(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<SubscriptionDeliveryQueueState>,
        metrics: Arc<SubscriptionDeliveryMetrics>,
    ) {
        let mut worker = self
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.shutdown.store(false, Ordering::Release);
        self.worker_start_count.fetch_add(1, Ordering::Relaxed);
        let runtime = Arc::downgrade(runtime);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("nimbus-subscription-delivery".to_string())
                .spawn(move || run_delivery_worker(runtime, queue, metrics, shutdown))
                .expect("subscription delivery worker should spawn"),
        );
    }

    pub(super) fn shutdown(&self, queue: &Arc<SubscriptionDeliveryQueueState>) {
        self.shutdown.store(true, Ordering::Release);
        queue.notify_all();
        if let Some(worker) = self
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .take()
        {
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
    }

    pub(super) fn running(&self) -> bool {
        self.worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .is_some()
    }

    pub(super) fn start_count(&self) -> u64 {
        self.worker_start_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
fn run_delivery_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<SubscriptionDeliveryQueueState>,
    metrics: Arc<SubscriptionDeliveryMetrics>,
    shutdown: Arc<AtomicBool>,
    pause: Option<Arc<SubscriptionDeliveryPauseState>>,
) {
    // Delivery intentionally uses a tenant-owned dedicated thread instead of
    // the shared Tokio background runtime. The key invariant is ownership:
    // this worker must outlive any request/task that enqueues delivery work,
    // remain explicitly bounded, and shut down via the tenant lifecycle.
    // The worker should not keep a tenant alive during deletion; the
    // explicit shutdown path joins first, and the weak upgrade lets the
    // worker exit cleanly if teardown wins the race.
    loop {
        let Some(first_work) = queue.pop_next(&shutdown) else {
            return;
        };

        if let Some(pause) = pause.as_ref() {
            pause.wait_if_armed();
        }

        let Some(mut work_batch) = queue.drain_ready_batch(&shutdown) else {
            return;
        };
        work_batch.insert(0, first_work);

        let (work, merged_count) = merge_queued_subscription_work(work_batch);
        if merged_count != 0 {
            metrics.record_queue_level_merge(merged_count);
        }

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let stats = dispatch_subscription_work(&runtime, &work);
        metrics.record_dispatch_stats(stats);
    }
}

#[cfg(not(test))]
fn run_delivery_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<SubscriptionDeliveryQueueState>,
    metrics: Arc<SubscriptionDeliveryMetrics>,
    shutdown: Arc<AtomicBool>,
) {
    // Delivery intentionally uses a tenant-owned dedicated thread instead of
    // the shared Tokio background runtime. The key invariant is ownership:
    // this worker must outlive any request/task that enqueues delivery work,
    // remain explicitly bounded, and shut down via the tenant lifecycle.
    // The worker should not keep a tenant alive during deletion; the
    // explicit shutdown path joins first, and the weak upgrade lets the
    // worker exit cleanly if teardown wins the race.
    loop {
        let Some(first_work) = queue.pop_next(&shutdown) else {
            return;
        };

        let Some(mut work_batch) = queue.drain_ready_batch(&shutdown) else {
            return;
        };
        work_batch.insert(0, first_work);

        let (work, merged_count) = merge_queued_subscription_work(work_batch);
        if merged_count != 0 {
            metrics.record_queue_level_merge(merged_count);
        }

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let stats = dispatch_subscription_work(&runtime, &work);
        metrics.record_dispatch_stats(stats);
    }
}
