use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord};
use tokio::sync::{mpsc, oneshot};

use crate::Engine;
use crate::engine::CommitPhaseDurations;

use super::super::{CommitterJob, QueuedMutationResult, TenantOperationGuard};
use super::CommitterPipelineMode;

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

#[derive(Clone, Copy, Default)]
pub(crate) struct PublisherErrorCounts {
    pub(crate) transient: u64,
    pub(crate) fatal: u64,
    pub(crate) ambiguous: u64,
}

pub(crate) enum PublisherMessage {
    Batch(AssignedPublisherBatch),
    Barrier(oneshot::Sender<()>),
    ResponseFence(Vec<DeferredPublisherResponse>),
    SerialJob {
        job: CommitterJob,
        drained: oneshot::Sender<()>,
    },
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
    pipeline_capable: bool,
    mode: AtomicU8,
    mode_transition_count: AtomicU64,
    requested_mode_override: AtomicU8,
}

impl PublisherHandoff {
    pub(crate) fn new(pipeline_capable: bool) -> Self {
        let capacity = env_positive_usize(
            "NIMBUS_COMMITTER_PUBLISHER_QUEUE_SIZE",
            DEFAULT_PUBLISHER_QUEUE_CAPACITY,
        );
        let send_timeout = Duration::from_millis(env_nonnegative_u64(
            "NIMBUS_COMMITTER_PUBLISHER_SEND_TIMEOUT_MS",
            DEFAULT_PUBLISHER_SEND_TIMEOUT_MS,
        ));
        let (sender, receiver) = mpsc::channel(capacity);
        let initial_mode = if pipeline_capable && pipeline_requested_from_env() {
            CommitterPipelineMode::Pipeline
        } else {
            CommitterPipelineMode::Serial
        };
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
            pipeline_capable,
            mode: AtomicU8::new(mode_to_u8(initial_mode)),
            mode_transition_count: AtomicU64::new(0),
            requested_mode_override: AtomicU8::new(0),
        }
    }

    /// Reconciles the requested per-tenant mode at the actor boundary.
    ///
    /// Pipeline -> serial first publishes everything already handed off, then
    /// exposes `Serial`; serial -> pipeline installs an empty-queue barrier
    /// before exposing `Pipeline`. The actor awaits this state machine before
    /// assigning the next batch, so no batch can straddle the two persistence
    /// owners.
    pub(crate) async fn reconcile_mode(&self) -> Result<bool> {
        let desired_pipeline = self.pipeline_capable && self.pipeline_requested();
        let current = self.mode();
        match (current, desired_pipeline) {
            (CommitterPipelineMode::Pipeline, false) => {
                self.mode.store(
                    mode_to_u8(CommitterPipelineMode::DrainingToSerial),
                    Ordering::Release,
                );
                if let Err(error) = self.barrier().await {
                    self.mode.store(
                        mode_to_u8(CommitterPipelineMode::Pipeline),
                        Ordering::Release,
                    );
                    return Err(error);
                }
                self.mode
                    .store(mode_to_u8(CommitterPipelineMode::Serial), Ordering::Release);
                self.mode_transition_count.fetch_add(1, Ordering::Relaxed);
                Ok(false)
            }
            (CommitterPipelineMode::Serial, true) => {
                self.mode.store(
                    mode_to_u8(CommitterPipelineMode::DrainingToPipeline),
                    Ordering::Release,
                );
                if let Err(error) = self.barrier().await {
                    self.mode
                        .store(mode_to_u8(CommitterPipelineMode::Serial), Ordering::Release);
                    return Err(error);
                }
                self.mode.store(
                    mode_to_u8(CommitterPipelineMode::Pipeline),
                    Ordering::Release,
                );
                self.mode_transition_count.fetch_add(1, Ordering::Relaxed);
                Ok(true)
            }
            (CommitterPipelineMode::Pipeline, true) => Ok(true),
            (CommitterPipelineMode::Serial, false) => Ok(false),
            (CommitterPipelineMode::DrainingToSerial, _)
            | (CommitterPipelineMode::DrainingToPipeline, _) => Err(Error::Internal(
                "committer pipeline transition re-entered before its drain completed".to_string(),
            )),
        }
    }

    pub(crate) fn mode(&self) -> CommitterPipelineMode {
        mode_from_u8(self.mode.load(Ordering::Acquire))
    }

    pub(crate) fn mode_transition_count(&self) -> u64 {
        self.mode_transition_count.load(Ordering::Relaxed)
    }

    fn pipeline_requested(&self) -> bool {
        match self.requested_mode_override.load(Ordering::Acquire) {
            1 => true,
            2 => false,
            _ => pipeline_requested_from_env(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_pipeline_requested_for_testing(&self, enabled: bool) {
        self.requested_mode_override
            .store(if enabled { 1 } else { 2 }, Ordering::Release);
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

    pub(crate) async fn send_serial_job(
        &self,
        job: CommitterJob,
    ) -> std::result::Result<oneshot::Receiver<()>, (CommitterJob, Error)> {
        match tokio::time::timeout(self.send_timeout, self.sender.reserve()).await {
            Ok(Ok(permit)) => {
                let (drained, wait_for_drain) = oneshot::channel();
                permit.send(PublisherMessage::SerialJob { job, drained });
                Ok(wait_for_drain)
            }
            Ok(Err(_)) => Err((
                job,
                Error::Internal("tenant publisher stopped before accepting serial job".to_string()),
            )),
            Err(_) => {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Err((
                    job,
                    Error::committer_full(
                        format!(
                            "tenant publisher queue remained full for {} ms (capacity {})",
                            self.send_timeout.as_millis(),
                            self.capacity
                        ),
                        self.capacity,
                    ),
                ))
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

    pub(crate) fn error_counts(&self) -> PublisherErrorCounts {
        PublisherErrorCounts {
            transient: self.transient_error_count.load(Ordering::Relaxed),
            fatal: self.fatal_error_count.load(Ordering::Relaxed),
            ambiguous: self.ambiguous_error_count.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn restore_error_counts(&self, counts: PublisherErrorCounts) {
        self.transient_error_count
            .store(counts.transient, Ordering::Relaxed);
        self.fatal_error_count
            .store(counts.fatal, Ordering::Relaxed);
        self.ambiguous_error_count
            .store(counts.ambiguous, Ordering::Relaxed);
    }
}

fn pipeline_requested_from_env() -> bool {
    !std::env::var("NIMBUS_COMMITTER_PIPELINE")
        .ok()
        .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "off" | "serial"))
}

const fn mode_to_u8(mode: CommitterPipelineMode) -> u8 {
    match mode {
        CommitterPipelineMode::Pipeline => 0,
        CommitterPipelineMode::DrainingToSerial => 1,
        CommitterPipelineMode::Serial => 2,
        CommitterPipelineMode::DrainingToPipeline => 3,
    }
}

fn mode_from_u8(mode: u8) -> CommitterPipelineMode {
    match mode {
        0 => CommitterPipelineMode::Pipeline,
        1 => CommitterPipelineMode::DrainingToSerial,
        2 => CommitterPipelineMode::Serial,
        3 => CommitterPipelineMode::DrainingToPipeline,
        _ => unreachable!("publisher mode atomic contains an invalid state"),
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
