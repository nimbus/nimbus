use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::subscriptions::{dispatch_subscription_work, merge_queued_subscription_work};
use crate::tenant::background::BackgroundWorker;

#[cfg(test)]
use super::pause::SubscriptionDeliveryPauseState;
use super::queue::SubscriptionDeliveryQueueState;
use super::stats::SubscriptionDeliveryMetrics;
use crate::tenant::TenantRuntime;

pub(super) struct SubscriptionDeliveryWorker {
    worker: BackgroundWorker,
}

impl SubscriptionDeliveryWorker {
    pub(super) fn new() -> Self {
        Self {
            worker: BackgroundWorker::new(),
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

    fn start_inner(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<SubscriptionDeliveryQueueState>,
        metrics: Arc<SubscriptionDeliveryMetrics>,
        #[cfg(test)] pause: Option<Arc<SubscriptionDeliveryPauseState>>,
    ) {
        let runtime = Arc::downgrade(runtime);
        self.worker
            .start("nimbus-subscription-delivery", move |shutdown| {
                run_delivery_worker(
                    runtime,
                    queue,
                    metrics,
                    shutdown,
                    #[cfg(test)]
                    pause,
                )
            });
    }

    #[cfg(test)]
    pub(super) fn shutdown(
        &self,
        queue: &Arc<SubscriptionDeliveryQueueState>,
        pause: &Arc<SubscriptionDeliveryPauseState>,
    ) {
        let queue = queue.clone();
        let pause = pause.clone();
        // Signal shutdown *before* releasing a worker parked in the test
        // pause barrier: `BackgroundWorker::shutdown` runs this closure
        // synchronously before it joins, so both orderings the two
        // mechanisms need are satisfied by this single sequence — the flag
        // is visible before the paused worker wakes (it won't process or
        // dispatch the batch it paused before draining), and the worker is
        // released before `shutdown` attempts to join it (no deadlock on a
        // still-paused worker).
        self.worker.shutdown(move |shutdown| {
            queue.signal_shutdown(shutdown);
            pause.release_for_shutdown();
        });
    }

    #[cfg(not(test))]
    pub(super) fn shutdown(&self, queue: &Arc<SubscriptionDeliveryQueueState>) {
        let queue = queue.clone();
        self.worker
            .shutdown(move |shutdown| queue.signal_shutdown(shutdown));
    }

    pub(super) fn running(&self) -> bool {
        self.worker.running()
    }

    pub(super) fn start_count(&self) -> u64 {
        self.worker.start_count()
    }
}

fn run_delivery_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<SubscriptionDeliveryQueueState>,
    metrics: Arc<SubscriptionDeliveryMetrics>,
    shutdown: Arc<AtomicBool>,
    #[cfg(test)] pause: Option<Arc<SubscriptionDeliveryPauseState>>,
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

        #[cfg(test)]
        if let Some(pause) = pause.as_ref() {
            pause.wait_if_armed(());
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
