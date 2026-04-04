use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
#[cfg(test)]
use std::time::Instant;

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionDispatchStats, dispatch_subscription_work,
    merge_queued_subscription_work,
};

use super::TenantRuntime;

pub(crate) const DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY: usize = 256;
const SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE: usize = 8;

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct SubscriptionDeliveryPauseHandle {
    state: Arc<SubscriptionDeliveryPauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct SubscriptionDeliveryPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct SubscriptionDeliveryPauseState {
    control: Mutex<SubscriptionDeliveryPauseControl>,
    condvar: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SubscriptionDeliveryStats {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub worker_running: bool,
    pub worker_start_count: u64,
    pub worker_restart_count: u64,
    pub overflow_sync_fallback_count: u64,
    pub coalesced_batch_count: u64,
    pub coalesced_commit_count: u64,
    pub merged_subscription_wakeup_count: u64,
    pub queue_level_merge_count: u64,
    pub coalesced_work_count: u64,
    pub reevaluation_count: u64,
    pub total_reevaluation_nanos: u64,
}

struct SubscriptionDeliveryState {
    queue: Mutex<VecDeque<QueuedSubscriptionWork>>,
    queue_ready: Condvar,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: AtomicBool,
    capacity: AtomicUsize,
    worker_start_count: AtomicU64,
    overflow_sync_fallback_count: AtomicU64,
    coalesced_batch_count: AtomicU64,
    coalesced_commit_count: AtomicU64,
    merged_subscription_wakeup_count: AtomicU64,
    queue_level_merge_count: AtomicU64,
    coalesced_work_count: AtomicU64,
    reevaluation_count: AtomicU64,
    total_reevaluation_nanos: AtomicU64,
    #[cfg(test)]
    pause: Arc<SubscriptionDeliveryPauseState>,
}

pub(super) struct SubscriptionDeliveryQueue {
    state: Arc<SubscriptionDeliveryState>,
}

#[cfg(test)]
impl SubscriptionDeliveryPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        control.armed = true;
        control.entered = false;
        control.released = false;
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        while control.armed && !control.entered {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let (next_control, wait_result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("subscription delivery pause wait should not be poisoned");
            control = next_control;
            if wait_result.timed_out() {
                return control.entered;
            }
        }
        control.entered
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(test)]
impl SubscriptionDeliveryPauseState {
    fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("subscription delivery pause wait should not be poisoned");
        }
        *control = SubscriptionDeliveryPauseControl::default();
    }
}

impl SubscriptionDeliveryQueue {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(SubscriptionDeliveryState {
                queue: Mutex::new(VecDeque::new()),
                queue_ready: Condvar::new(),
                worker: Mutex::new(None),
                shutdown: AtomicBool::new(false),
                capacity: AtomicUsize::new(DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY),
                worker_start_count: AtomicU64::new(0),
                overflow_sync_fallback_count: AtomicU64::new(0),
                coalesced_batch_count: AtomicU64::new(0),
                coalesced_commit_count: AtomicU64::new(0),
                merged_subscription_wakeup_count: AtomicU64::new(0),
                queue_level_merge_count: AtomicU64::new(0),
                coalesced_work_count: AtomicU64::new(0),
                reevaluation_count: AtomicU64::new(0),
                total_reevaluation_nanos: AtomicU64::new(0),
                #[cfg(test)]
                pause: Arc::new(SubscriptionDeliveryPauseState::default()),
            }),
        }
    }

    pub(super) fn start_worker(&self, runtime: &Arc<TenantRuntime>) {
        let mut worker = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.state.shutdown.store(false, Ordering::Release);
        self.state
            .worker_start_count
            .fetch_add(1, Ordering::Relaxed);
        let state = self.state.clone();
        // Delivery intentionally uses a tenant-owned dedicated thread instead of
        // the shared Tokio background runtime. The key invariant is ownership:
        // this worker must outlive any request/task that enqueues delivery work,
        // remain explicitly bounded, and shut down via the tenant lifecycle.
        // The worker should not keep a tenant alive during deletion; the
        // explicit shutdown path joins first, and the weak upgrade lets the
        // worker exit cleanly if teardown wins the race.
        let runtime = Arc::downgrade(runtime);
        *worker =
            Some(
                std::thread::Builder::new()
                    .name("neovex-subscription-delivery".to_string())
                    .spawn(move || {
                        loop {
                            let first_work = {
                                let mut queue = state.queue.lock().expect(
                                    "subscription delivery queue lock should not be poisoned",
                                );
                                loop {
                                    if state.shutdown.load(Ordering::Acquire) {
                                        queue.clear();
                                        return;
                                    }
                                    if let Some(work) = queue.pop_front() {
                                        break work;
                                    }
                                    queue = state.queue_ready.wait(queue).expect(
                                        "subscription delivery wait should not be poisoned",
                                    );
                                }
                            };

                            #[cfg(test)]
                            state.pause.wait_if_armed();

                            let mut work_batch = vec![first_work];
                            {
                                let mut queue = state.queue.lock().expect(
                                    "subscription delivery queue lock should not be poisoned",
                                );
                                if state.shutdown.load(Ordering::Acquire) {
                                    queue.clear();
                                    return;
                                }
                                while work_batch.len() < SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE {
                                    let Some(work) = queue.pop_front() else {
                                        break;
                                    };
                                    work_batch.push(work);
                                }
                            }
                            let (work, merged_count) = merge_queued_subscription_work(work_batch);
                            if merged_count != 0 {
                                state
                                    .queue_level_merge_count
                                    .fetch_add(merged_count, Ordering::Relaxed);
                            }

                            let Some(runtime) = runtime.upgrade() else {
                                return;
                            };
                            let stats = dispatch_subscription_work(&runtime, &work);
                            state.record_dispatch_stats(stats);
                        }
                    })
                    .expect("subscription delivery worker should spawn"),
            );
    }

    pub(super) fn enqueue(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        let mut queue = self
            .state
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        if queue.len() >= self.state.capacity.load(Ordering::Acquire).max(1) {
            return Err(work);
        }
        queue.push_back(work);
        self.state.queue_ready.notify_one();
        Ok(())
    }

    pub(super) fn record_overflow_sync_fallback(&self) {
        self.state
            .overflow_sync_fallback_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.state
            .coalesced_batch_count
            .fetch_add(1, Ordering::Relaxed);
        self.state
            .coalesced_commit_count
            .fetch_add(commit_count, Ordering::Relaxed);
        self.state
            .merged_subscription_wakeup_count
            .fetch_add(merged_subscription_wakeup_count, Ordering::Relaxed);
    }

    pub(super) fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.state.record_dispatch_stats(stats);
    }

    pub(super) fn shutdown(&self) {
        self.state.shutdown.store(true, Ordering::Release);
        self.state.queue_ready.notify_all();
        if let Some(worker) = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .take()
        {
            // Cleanup can be triggered from inside delivery paths; skip
            // joining ourselves and let the thread return naturally instead.
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
    }

    pub(super) fn stats(&self) -> SubscriptionDeliveryStats {
        let queue = self
            .state
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        let oldest_queue_age_nanos = queue
            .front()
            .map(|work| work.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let worker_running = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .is_some();
        let worker_start_count = self.state.worker_start_count.load(Ordering::Relaxed);
        SubscriptionDeliveryStats {
            queue_depth: queue.len(),
            queue_capacity: self.state.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            worker_running,
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            overflow_sync_fallback_count: self
                .state
                .overflow_sync_fallback_count
                .load(Ordering::Relaxed),
            coalesced_batch_count: self.state.coalesced_batch_count.load(Ordering::Relaxed),
            coalesced_commit_count: self.state.coalesced_commit_count.load(Ordering::Relaxed),
            merged_subscription_wakeup_count: self
                .state
                .merged_subscription_wakeup_count
                .load(Ordering::Relaxed),
            queue_level_merge_count: self.state.queue_level_merge_count.load(Ordering::Relaxed),
            coalesced_work_count: self.state.coalesced_work_count.load(Ordering::Relaxed),
            reevaluation_count: self.state.reevaluation_count.load(Ordering::Relaxed),
            total_reevaluation_nanos: self.state.total_reevaluation_nanos.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(super) fn set_capacity_for_testing(&self, capacity: usize) {
        self.state
            .capacity
            .store(capacity.max(1), Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn pause_handle(&self) -> SubscriptionDeliveryPauseHandle {
        SubscriptionDeliveryPauseHandle {
            state: self.state.pause.clone(),
        }
    }
}

impl SubscriptionDeliveryState {
    fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.coalesced_work_count
            .fetch_add(stats.coalesced_work_count, Ordering::Relaxed);
        self.reevaluation_count
            .fetch_add(stats.reevaluation_count, Ordering::Relaxed);
        self.total_reevaluation_nanos
            .fetch_add(stats.total_reevaluation_nanos, Ordering::Relaxed);
    }
}

impl Drop for SubscriptionDeliveryQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}
