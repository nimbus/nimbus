use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord};
use tokio::sync::{mpsc, oneshot};

use crate::Engine;
use crate::engine::CommitPhaseDurations;

use super::super::{QueuedMutationResult, TenantOperationGuard};

const DEFAULT_PUBLISHER_QUEUE_CAPACITY: usize = 32;
const DEFAULT_PUBLISHER_SEND_TIMEOUT_MS: u64 = 500;

pub(crate) struct PendingPublisherResponse {
    pub(crate) _operation: TenantOperationGuard,
    pub(crate) response: oneshot::Sender<Result<QueuedMutationResult>>,
    pub(crate) result: QueuedMutationResult,
}

pub(crate) struct DeferredPublisherResponse {
    pub(crate) _operation: TenantOperationGuard,
    pub(crate) response: oneshot::Sender<Result<QueuedMutationResult>>,
    pub(crate) result: Result<QueuedMutationResult>,
}

impl DeferredPublisherResponse {
    pub(crate) fn complete(self) {
        let _ = self.response.send(self.result);
    }

    pub(crate) fn fail(self, error: &Error) {
        let _ = self.response.send(Err(error.clone()));
    }
}

pub(crate) struct AssignedPublisherBatch {
    pub(crate) engine: Arc<Engine>,
    pub(crate) records: Arc<Vec<TenantEventRecord>>,
    pub(crate) responses: Vec<PendingPublisherResponse>,
    pub(crate) phases: CommitPhaseDurations,
    pub(crate) sample_started_at: Instant,
}

impl AssignedPublisherBatch {
    pub(crate) fn first_sequence(&self) -> SequenceNumber {
        self.records
            .first()
            .expect("assigned publisher batches must not be empty")
            .sequence
    }

    pub(crate) fn last_sequence(&self) -> SequenceNumber {
        self.records
            .last()
            .expect("assigned publisher batches must not be empty")
            .sequence
    }

    pub(crate) fn fail(self, error: &Error) {
        for pending in self.responses {
            let _ = pending.response.send(Err(error.clone()));
        }
    }

    pub(crate) fn merge(&mut self, mut next: Self) {
        debug_assert!(Arc::ptr_eq(&self.engine, &next.engine));
        debug_assert_eq!(
            self.last_sequence().0.checked_add(1),
            Some(next.first_sequence().0),
            "publisher accumulator may merge only contiguous assigned batches"
        );
        Arc::make_mut(&mut self.records).extend(next.records.iter().cloned());
        self.responses.append(&mut next.responses);
        self.phases.merge_assignment(next.phases);
        self.sample_started_at = self.sample_started_at.min(next.sample_started_at);
    }
}

pub(crate) type PublisherQueueError = Box<(AssignedPublisherBatch, Error)>;

pub(crate) enum PublisherMessage {
    Batch(AssignedPublisherBatch),
    Barrier(oneshot::Sender<()>),
    ResponseFence(Vec<DeferredPublisherResponse>),
}

pub(crate) struct PublisherHandoff {
    sender: mpsc::Sender<PublisherMessage>,
    receiver: Mutex<Option<mpsc::Receiver<PublisherMessage>>>,
    started: AtomicBool,
    capacity: usize,
    send_timeout: Duration,
    send_timeout_count: AtomicU64,
    transient_error_count: AtomicU64,
    fatal_error_count: AtomicU64,
    ambiguous_error_count: AtomicU64,
}

impl PublisherHandoff {
    pub(crate) fn new() -> Self {
        let capacity = env_positive_usize(
            "NIMBUS_COMMITTER_PUBLISHER_QUEUE_SIZE",
            DEFAULT_PUBLISHER_QUEUE_CAPACITY,
        );
        let send_timeout = Duration::from_millis(env_nonnegative_u64(
            "NIMBUS_COMMITTER_PUBLISHER_SEND_TIMEOUT_MS",
            DEFAULT_PUBLISHER_SEND_TIMEOUT_MS,
        ));
        let (sender, receiver) = mpsc::channel(capacity);
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            started: AtomicBool::new(false),
            capacity,
            send_timeout,
            send_timeout_count: AtomicU64::new(0),
            transient_error_count: AtomicU64::new(0),
            fatal_error_count: AtomicU64::new(0),
            ambiguous_error_count: AtomicU64::new(0),
        }
    }

    pub(crate) fn take_receiver(&self) -> mpsc::Receiver<PublisherMessage> {
        assert!(
            !self.started.swap(true, Ordering::AcqRel),
            "tenant publisher must be started exactly once"
        );
        self.receiver
            .lock()
            .expect("publisher receiver lock should not be poisoned")
            .take()
            .expect("publisher receiver should exist before task start")
    }

    pub(crate) async fn send(
        &self,
        batch: AssignedPublisherBatch,
    ) -> std::result::Result<(), PublisherQueueError> {
        match tokio::time::timeout(self.send_timeout, self.sender.reserve()).await {
            Ok(Ok(permit)) => {
                permit.send(PublisherMessage::Batch(batch));
                Ok(())
            }
            Ok(Err(_)) => Err(Box::new((
                batch,
                Error::Internal(
                    "tenant publisher stopped before accepting assigned batch".to_string(),
                ),
            ))),
            Err(_) => {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Err(Box::new((
                    batch,
                    Error::committer_full(
                        format!(
                            "tenant publisher queue remained full for {} ms (capacity {})",
                            self.send_timeout.as_millis(),
                            self.capacity
                        ),
                        self.capacity,
                    ),
                )))
            }
        }
    }

    pub(crate) async fn barrier(&self) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        let permit = tokio::time::timeout(self.send_timeout, self.sender.reserve())
            .await
            .map_err(|_| {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Error::committer_full(
                    format!(
                        "tenant publisher barrier queue remained full for {} ms (capacity {})",
                        self.send_timeout.as_millis(),
                        self.capacity
                    ),
                    self.capacity,
                )
            })?
            .map_err(|_| {
                Error::Internal(
                    "tenant publisher stopped before accepting serial barrier".to_string(),
                )
            })?;
        permit.send(PublisherMessage::Barrier(sender));
        receiver.await.map_err(|_| {
            Error::Internal("tenant publisher stopped before completing serial barrier".to_string())
        })
    }

    pub(crate) async fn send_response_fence(
        &self,
        responses: Vec<DeferredPublisherResponse>,
    ) -> std::result::Result<(), Box<(Vec<DeferredPublisherResponse>, Error)>> {
        if responses.is_empty() {
            return Ok(());
        }
        match tokio::time::timeout(self.send_timeout, self.sender.reserve()).await {
            Ok(Ok(permit)) => {
                permit.send(PublisherMessage::ResponseFence(responses));
                Ok(())
            }
            Ok(Err(_)) => Err(Box::new((
                responses,
                Error::Internal(
                    "tenant publisher stopped before accepting response fence".to_string(),
                ),
            ))),
            Err(_) => {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Err(Box::new((
                    responses,
                    Error::committer_full(
                        format!(
                            "tenant publisher queue remained full for {} ms (capacity {})",
                            self.send_timeout.as_millis(),
                            self.capacity
                        ),
                        self.capacity,
                    ),
                )))
            }
        }
    }

    pub(crate) fn depth(&self) -> usize {
        self.sender
            .max_capacity()
            .saturating_sub(self.sender.capacity())
    }

    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn send_timeout_count(&self) -> u64 {
        self.send_timeout_count.load(Ordering::Relaxed)
    }

    pub(crate) fn record_transient_error(&self) {
        self.transient_error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fatal_error(&self) {
        self.fatal_error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_ambiguous_error(&self) {
        self.ambiguous_error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn error_counts(&self) -> (u64, u64, u64) {
        (
            self.transient_error_count.load(Ordering::Relaxed),
            self.fatal_error_count.load(Ordering::Relaxed),
            self.ambiguous_error_count.load(Ordering::Relaxed),
        )
    }
}

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_nonnegative_u64(key: &str, default: u64) -> u64 {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}
