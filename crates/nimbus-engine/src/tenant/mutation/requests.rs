use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use nimbus_core::{DependencySet, DocumentId, Result, SequenceNumber};
use tokio::sync::oneshot;

use crate::engine::PreparedCommit;

use super::super::{TenantOperationGuard, TenantRuntime};

pub(crate) const DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY: usize = 256;

pub(crate) enum QueuedMutationResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(crate) struct PreparedPayloadAccounting {
    runtime: Arc<TenantRuntime>,
    bytes: u64,
}

impl PreparedPayloadAccounting {
    pub(crate) fn new(runtime: Arc<TenantRuntime>, bytes: u64) -> Self {
        runtime
            .commit_phase_metrics()
            .accept_prepared_payload(bytes);
        Self { runtime, bytes }
    }
}

impl Drop for PreparedPayloadAccounting {
    fn drop(&mut self) {
        self.runtime
            .commit_phase_metrics()
            .release_prepared_payload(self.bytes);
    }
}

pub(crate) struct QueuedMutationRequest {
    pub prepared_commit: Box<PreparedCommit>,
    pub conflict_dependencies: DependencySet,
    pub result: QueuedMutationResult,
    pub prepared_payload_accounting: Option<PreparedPayloadAccounting>,
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
