use nimbus_core::SequenceNumber;
use serde::Serialize;

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
#[serde(rename_all = "kebab-case")]
pub enum CommitterPipelineMode {
    Pipeline,
    DrainingToSerial,
    Serial,
    DrainingToPipeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationJournalStats {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub apply_lag: u64,
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
    pub publisher_queue_depth: usize,
    pub publisher_queue_capacity: usize,
    pub publisher_send_timeout_count: u64,
    pub publisher_transient_error_count: u64,
    pub publisher_fatal_error_count: u64,
    pub publisher_ambiguous_error_count: u64,
    pub publisher_mode: CommitterPipelineMode,
    pub publisher_mode_transition_count: u64,
    pub publisher_mode_transition_failure_count: u64,
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
    pub observer_spawned_work_poisoned: bool,
}
