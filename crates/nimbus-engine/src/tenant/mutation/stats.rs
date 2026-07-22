use nimbus_core::Timestamp;
use serde::Serialize;

use super::frontiers::MutationFrontierStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationAdmissionPhase {
    Idle,
    Dropping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationAdmissionStats {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub admitted_count: u64,
    pub shed_count: u64,
    pub queue_rejection_count: u64,
    pub codel_phase: MutationAdmissionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationIsolateAdmissionStats {
    pub concurrent_count: usize,
    pub ceiling: usize,
    pub waiting_count: usize,
    pub waiting_capacity: usize,
    pub max_concurrent_count: usize,
    pub admitted_count: u64,
    pub shed_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationJournalStats {
    #[serde(flatten)]
    pub frontiers: MutationFrontierStats,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub pending_response_count: u64,
    pub worker_running: bool,
    pub worker_start_count: u64,
    pub worker_restart_count: u64,
    pub queue_rejection_count: u64,
    pub worker_failure_count: u64,
    pub read_wait_count: u64,
    pub total_read_wait_nanos: u64,
    pub committer_inbox_depth: usize,
    pub committer_inbox_capacity: usize,
    pub committer_send_timeout_millis: u64,
    pub committer_send_timeout_count: u64,
    pub committer_lease_acquired: bool,
    pub committer_lease_epoch: u64,
    pub committer_lease_expires_at: Timestamp,
    pub committer_lease_fenced: bool,
    pub committer_lease_acquire_count: u64,
    pub committer_lease_renewal_count: u64,
    pub committer_lease_renewal_failure_count: u64,
    pub committer_lease_renewal_failure_streak: u64,
    pub committer_lease_last_success_age_millis: Option<u64>,
    pub committer_lease_renewal_worker_running: bool,
    pub publisher_queue_depth: usize,
    pub publisher_queue_capacity: usize,
    pub publisher_send_timeout_count: u64,
    pub publisher_transient_error_count: u64,
    pub publisher_fatal_error_count: u64,
    pub publisher_ambiguous_error_count: u64,
    pub committer_arm: super::CommitterArm,
    pub observer_queue_depth: usize,
    /// Largest observer queue depth reserved by this tenant runtime.
    pub observer_queue_peak_depth: usize,
    pub observer_queue_capacity: usize,
    pub observer_queue_high_watermark: usize,
    pub observer_queue_high_water_warning_count: u64,
    pub observer_queue_cap_breach_count: u64,
    pub observer_catch_up_enqueue_failure_count: u64,
    pub provider_catch_up_failure_count: u64,
    pub observer_dispatch_poisoned: bool,
    pub observer_spawned_work_depth: usize,
    pub observer_spawned_work_capacity: usize,
    pub observer_spawned_work_high_watermark: usize,
    pub observer_spawned_work_high_water_warning_count: u64,
    pub observer_spawned_work_cap_breach_count: u64,
    pub observer_spawned_work_dropped_event_count: u64,
    pub observer_spawned_work_dirty_scope_count: usize,
    pub observer_spawned_work_token_lag_scope_count: usize,
    pub observer_spawned_work_stale_no_op_count: u64,
    pub observer_spawned_work_delayed_retry_count: u64,
    pub observer_spawned_work_consecutive_failure_count: u32,
    pub observer_spawned_work_current_retry_backoff_millis: u64,
    pub observer_spawned_work_reconciliation_retry_count: u64,
    pub observer_spawned_work_current_reconciliation_backoff_millis: u64,
    pub observer_spawned_work_poisoned: bool,
}

impl std::ops::Deref for MutationJournalStats {
    type Target = MutationFrontierStats;

    fn deref(&self) -> &Self::Target {
        &self.frontiers
    }
}
