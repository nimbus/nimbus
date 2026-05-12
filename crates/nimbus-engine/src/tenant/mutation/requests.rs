use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use nimbus_core::{DocumentId, Mutation, PrincipalContext, Result};
use tokio::sync::oneshot;

use super::super::TenantOperationGuard;

pub(crate) const DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY: usize = 256;

pub(crate) enum QueuedMutationResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(crate) struct QueuedMutationRequest {
    pub mutation: Mutation,
    pub principal: PrincipalContext,
    pub scheduled_execution_id: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub _operation: TenantOperationGuard,
    pub response: oneshot::Sender<Result<QueuedMutationResult>>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub enqueued_at: Instant,
}
