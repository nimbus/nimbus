use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use nimbus_core::{DependencySet, DocumentId, Result, SequenceNumber};
use tokio::sync::oneshot;

use crate::engine::PreparedCommit;

use super::super::TenantOperationGuard;

pub(crate) const DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY: usize = 256;

pub(crate) enum QueuedMutationResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(crate) struct QueuedMutationRequest {
    pub prepared_commit: PreparedCommit,
    pub conflict_dependencies: DependencySet,
    pub result: QueuedMutationResult,
    pub prepare_nanos: u64,
    pub cancelled: Arc<AtomicBool>,
    pub _operation: TenantOperationGuard,
    pub response: oneshot::Sender<Result<QueuedMutationResult>>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub enqueued_at: Instant,
    /// Durable head observed at admission, used only by path A's shadow OCC
    /// instrumentation. Real serialization continues to use the head sampled
    /// on the serial committer.
    pub shadow_snapshot_sequence: SequenceNumber,
}
