use neovex_core::SequenceNumber;
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
}
