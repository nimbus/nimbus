use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::subscriptions::SubscriptionDispatchStats;

use super::queue::SubscriptionDeliveryQueueState;

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

pub(super) struct SubscriptionDeliveryMetrics {
    overflow_sync_fallback_count: AtomicU64,
    coalesced_batch_count: AtomicU64,
    coalesced_commit_count: AtomicU64,
    merged_subscription_wakeup_count: AtomicU64,
    queue_level_merge_count: AtomicU64,
    coalesced_work_count: AtomicU64,
    reevaluation_count: AtomicU64,
    total_reevaluation_nanos: AtomicU64,
}

impl SubscriptionDeliveryMetrics {
    pub(super) fn new() -> Self {
        Self {
            overflow_sync_fallback_count: AtomicU64::new(0),
            coalesced_batch_count: AtomicU64::new(0),
            coalesced_commit_count: AtomicU64::new(0),
            merged_subscription_wakeup_count: AtomicU64::new(0),
            queue_level_merge_count: AtomicU64::new(0),
            coalesced_work_count: AtomicU64::new(0),
            reevaluation_count: AtomicU64::new(0),
            total_reevaluation_nanos: AtomicU64::new(0),
        }
    }

    pub(super) fn record_overflow_sync_fallback(&self) {
        self.overflow_sync_fallback_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.coalesced_batch_count.fetch_add(1, Ordering::Relaxed);
        self.coalesced_commit_count
            .fetch_add(commit_count, Ordering::Relaxed);
        self.merged_subscription_wakeup_count
            .fetch_add(merged_subscription_wakeup_count, Ordering::Relaxed);
    }

    pub(super) fn record_queue_level_merge(&self, merged_count: u64) {
        self.queue_level_merge_count
            .fetch_add(merged_count, Ordering::Relaxed);
    }

    pub(super) fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.coalesced_work_count
            .fetch_add(stats.coalesced_work_count, Ordering::Relaxed);
        self.reevaluation_count
            .fetch_add(stats.reevaluation_count, Ordering::Relaxed);
        self.total_reevaluation_nanos
            .fetch_add(stats.total_reevaluation_nanos, Ordering::Relaxed);
    }

    pub(super) fn snapshot(
        &self,
        queue: &Arc<SubscriptionDeliveryQueueState>,
        worker_running: bool,
        worker_start_count: u64,
    ) -> SubscriptionDeliveryStats {
        let queue = queue.snapshot();
        SubscriptionDeliveryStats {
            queue_depth: queue.depth,
            queue_capacity: queue.capacity,
            oldest_queue_age_nanos: queue.oldest_queue_age_nanos,
            worker_running,
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            overflow_sync_fallback_count: self.overflow_sync_fallback_count.load(Ordering::Relaxed),
            coalesced_batch_count: self.coalesced_batch_count.load(Ordering::Relaxed),
            coalesced_commit_count: self.coalesced_commit_count.load(Ordering::Relaxed),
            merged_subscription_wakeup_count: self
                .merged_subscription_wakeup_count
                .load(Ordering::Relaxed),
            queue_level_merge_count: self.queue_level_merge_count.load(Ordering::Relaxed),
            coalesced_work_count: self.coalesced_work_count.load(Ordering::Relaxed),
            reevaluation_count: self.reevaluation_count.load(Ordering::Relaxed),
            total_reevaluation_nanos: self.total_reevaluation_nanos.load(Ordering::Relaxed),
        }
    }
}
