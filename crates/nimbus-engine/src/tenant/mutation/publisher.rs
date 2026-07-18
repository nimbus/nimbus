use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord, TenantId};
use tokio::sync::{mpsc, oneshot};

use crate::Engine;
use crate::engine::CommitPhaseDurations;
use crate::engine::committed_mutations::{
    CommittedMutationObserverDispatch, CommittedMutationObserverMessage,
};

use super::super::{
    CommitterJob, MutationResponseSender, QueuedMutationResult, TenantOperationGuard,
};
use super::CommitterPipelineMode;

const DEFAULT_PUBLISHER_QUEUE_CAPACITY: usize = 32;
const DEFAULT_PUBLISHER_SEND_TIMEOUT_MS: u64 = 500;
const DEFAULT_OBSERVER_QUEUE_CAPACITY: usize = 4_096;
const DEFAULT_OBSERVER_QUEUE_HIGH_WATERMARK: usize = 3_072;

fn publisher_limits_from_env() -> (usize, Duration) {
    (
        crate::config::env_positive_usize(
            "NIMBUS_COMMITTER_PUBLISHER_QUEUE_SIZE",
            DEFAULT_PUBLISHER_QUEUE_CAPACITY,
        ),
        Duration::from_millis(crate::config::env_nonnegative_u64(
            "NIMBUS_COMMITTER_PUBLISHER_SEND_TIMEOUT_MS",
            DEFAULT_PUBLISHER_SEND_TIMEOUT_MS,
        )),
    )
}

fn observer_limits_from_env() -> (usize, usize, usize) {
    let requested_capacity = crate::config::env_positive_usize(
        "NIMBUS_COMMITTED_OBSERVER_QUEUE_CAPACITY",
        DEFAULT_OBSERVER_QUEUE_CAPACITY,
    );
    let requested_high_watermark = crate::config::env_positive_usize(
        "NIMBUS_COMMITTED_OBSERVER_QUEUE_HIGH_WATERMARK",
        DEFAULT_OBSERVER_QUEUE_HIGH_WATERMARK,
    );
    (
        requested_capacity,
        requested_high_watermark,
        crate::config::committer_publisher_batch_max(),
    )
}

fn clamp_observer_limits(
    tenant_id: &TenantId,
    requested_capacity: usize,
    requested_high_watermark: usize,
    max_dispatch_size: usize,
) -> (usize, usize) {
    let capacity = requested_capacity.max(1).max(max_dispatch_size.max(1));
    if capacity != requested_capacity {
        tracing::warn!(
            tenant = %tenant_id,
            requested_observer_queue_capacity = requested_capacity,
            max_committed_observer_dispatch_size = max_dispatch_size,
            observer_queue_capacity = capacity,
            "clamped committed mutation observer capacity to the maximum single dispatch"
        );
    }
    let high_watermark = requested_high_watermark.max(1).min(capacity);
    (capacity, high_watermark)
}

#[cfg(test)]
static PUBLISHER_LIMITS_FOR_TESTING: std::sync::OnceLock<
    Mutex<std::collections::HashMap<TenantId, (usize, Duration)>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn configure_publisher_limits_for_testing(
    tenant_id: TenantId,
    capacity: usize,
    send_timeout: Duration,
) {
    PUBLISHER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("publisher test-limit lock should not be poisoned")
        .insert(tenant_id, (capacity.max(1), send_timeout));
}

#[cfg(test)]
fn take_publisher_limits_for_testing(tenant_id: &TenantId) -> Option<(usize, Duration)> {
    PUBLISHER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("publisher test-limit lock should not be poisoned")
        .remove(tenant_id)
}

#[cfg(test)]
type ObserverLimitsForTesting = (usize, usize, usize);

#[cfg(test)]
type ObserverLimitOverrides = Mutex<std::collections::HashMap<TenantId, ObserverLimitsForTesting>>;

#[cfg(test)]
static OBSERVER_LIMITS_FOR_TESTING: std::sync::OnceLock<ObserverLimitOverrides> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn configure_observer_limits_for_testing(
    tenant_id: TenantId,
    capacity: usize,
    high_watermark: usize,
    max_dispatch_size: usize,
) {
    OBSERVER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("observer test-limit lock should not be poisoned")
        .insert(
            tenant_id,
            (
                capacity.max(1),
                high_watermark.max(1),
                max_dispatch_size.max(1),
            ),
        );
}

#[cfg(test)]
fn take_observer_limits_for_testing(tenant_id: &TenantId) -> Option<ObserverLimitsForTesting> {
    OBSERVER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("observer test-limit lock should not be poisoned")
        .remove(tenant_id)
}

pub(crate) struct ObserverHandoff {
    tenant_id: TenantId,
    sender: Mutex<ObserverSender>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<CommittedMutationObserverMessage>>>,
    queue_depth: AtomicUsize,
    queue_capacity: usize,
    queue_high_watermark: usize,
    high_water_warning_active: AtomicBool,
    high_water_warning_count: AtomicU64,
    cap_breach_count: AtomicU64,
    poisoned: AtomicBool,
    started: AtomicBool,
    drained: AtomicBool,
    drained_notify: tokio::sync::Notify,
}

struct ObserverSender {
    sender: mpsc::UnboundedSender<CommittedMutationObserverMessage>,
    closed: bool,
}

impl ObserverHandoff {
    pub(crate) fn new(tenant_id: &TenantId) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        #[cfg(test)]
        let requested_limits =
            take_observer_limits_for_testing(tenant_id).unwrap_or_else(observer_limits_from_env);
        #[cfg(not(test))]
        let requested_limits = observer_limits_from_env();
        let (queue_capacity, queue_high_watermark) = clamp_observer_limits(
            tenant_id,
            requested_limits.0,
            requested_limits.1,
            requested_limits.2,
        );
        Self {
            tenant_id: tenant_id.clone(),
            sender: Mutex::new(ObserverSender {
                sender,
                closed: false,
            }),
            receiver: Mutex::new(Some(receiver)),
            queue_depth: AtomicUsize::new(0),
            queue_capacity,
            queue_high_watermark,
            high_water_warning_active: AtomicBool::new(false),
            high_water_warning_count: AtomicU64::new(0),
            cap_breach_count: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            started: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            drained_notify: tokio::sync::Notify::new(),
        }
    }

    pub(crate) fn send(
        &self,
        mut dispatch: CommittedMutationObserverDispatch,
        runtime: std::sync::Weak<super::super::TenantRuntime>,
    ) -> Result<()> {
        let event_count = dispatch.event_count();
        let mut sender = self
            .sender
            .lock()
            .expect("observer sender lock should not be poisoned");
        if self.poisoned.load(Ordering::Acquire) {
            return Err(self.poisoned_error());
        }
        if sender.closed {
            self.poison_locked(
                &mut sender,
                "observer dispatch arrived after the ordered dispatcher close",
            );
            return Err(self.poisoned_error());
        }

        let depth = self.queue_depth.load(Ordering::Acquire);
        if event_count > self.queue_capacity.saturating_sub(depth) {
            self.cap_breach_count.fetch_add(1, Ordering::Relaxed);
            let reason = format!(
                "committed mutation observer queue hard cap breached: depth={depth}, incoming_events={event_count}, capacity={}",
                self.queue_capacity
            );
            self.poison_locked(&mut sender, &reason);
            return Err(self.poisoned_error());
        }

        let next_depth = depth + event_count;
        self.queue_depth.store(next_depth, Ordering::Release);
        dispatch.arm_completion(runtime);
        if next_depth >= self.queue_high_watermark
            && !self.high_water_warning_active.swap(true, Ordering::AcqRel)
        {
            self.high_water_warning_count
                .fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                tenant = %self.tenant_id,
                observer_queue_depth = next_depth,
                observer_queue_high_watermark = self.queue_high_watermark,
                observer_queue_capacity = self.queue_capacity,
                "committed mutation observer queue crossed its high-water mark"
            );
        }
        if let Err(rejected) = sender
            .sender
            .send(CommittedMutationObserverMessage::Dispatch(dispatch))
        {
            self.poison_locked(
                &mut sender,
                "committed mutation observer dispatcher stopped while accepting a dispatch",
            );
            drop(sender);
            // The dispatch owns its depth completion. Drop it only after the
            // sender lock is released so completion can update the handoff.
            drop(rejected);
            return Err(self.poisoned_error());
        }
        Ok(())
    }

    fn poison_locked(&self, sender: &mut ObserverSender, reason: &str) {
        if !self.poisoned.swap(true, Ordering::AcqRel) {
            tracing::error!(
                tenant = %self.tenant_id,
                observer_queue_depth = self.queue_depth.load(Ordering::Acquire),
                observer_queue_capacity = self.queue_capacity,
                reason,
                "committed mutation observer dispatcher poisoned; accepted work will drain and no new observer events will be accepted"
            );
        }
        if !std::mem::replace(&mut sender.closed, true)
            && sender
                .sender
                .send(CommittedMutationObserverMessage::Close)
                .is_err()
        {
            self.mark_drained();
        }
    }

    pub(crate) fn poison(&self, reason: &str) {
        let mut sender = self
            .sender
            .lock()
            .expect("observer sender lock should not be poisoned");
        self.poison_locked(&mut sender, reason);
    }

    fn poisoned_error(&self) -> Error {
        Error::Internal(format!(
            "committed mutation observer dispatcher is poisoned for tenant {}",
            self.tenant_id
        ))
    }

    pub(crate) fn close(&self) {
        let mut sender = self
            .sender
            .lock()
            .expect("observer sender lock should not be poisoned");
        if !std::mem::replace(&mut sender.closed, true)
            && sender
                .sender
                .send(CommittedMutationObserverMessage::Close)
                .is_err()
        {
            self.poisoned.store(true, Ordering::Release);
            tracing::error!(
                tenant = %self.tenant_id,
                "committed mutation observer dispatcher stopped before ordered close"
            );
            self.mark_drained();
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn fence(&self) -> Result<()> {
        let (completed, completion) = oneshot::channel();
        {
            let sender = self
                .sender
                .lock()
                .expect("observer sender lock should not be poisoned");
            if self.poisoned.load(Ordering::Acquire) {
                return Err(self.poisoned_error());
            }
            if sender.closed {
                return Err(Error::Internal(format!(
                    "committed mutation observer dispatcher is closed for tenant {}",
                    self.tenant_id
                )));
            }
            sender
                .sender
                .send(CommittedMutationObserverMessage::Fence(completed))
                .map_err(|_| {
                    Error::Internal(
                        "committed mutation observer dispatcher stopped before a fence".to_string(),
                    )
                })?;
        }
        completion.await.map_err(|_| {
            Error::Internal(
                "committed mutation observer dispatcher dropped a fence response".to_string(),
            )
        })
    }

    pub(crate) fn mark_drained(&self) {
        self.drained.store(true, Ordering::Release);
        self.drained_notify.notify_waiters();
    }

    pub(crate) fn complete_dispatch(&self, event_count: usize) {
        let _sender = self
            .sender
            .lock()
            .expect("observer sender lock should not be poisoned");
        self.complete_dispatch_locked(event_count);
    }

    fn complete_dispatch_locked(&self, event_count: usize) {
        let depth = self.queue_depth.load(Ordering::Acquire);
        debug_assert!(
            depth >= event_count,
            "observer queue depth cannot underflow on dispatch completion"
        );
        let next_depth = depth.saturating_sub(event_count);
        self.queue_depth.store(next_depth, Ordering::Release);
        if next_depth < self.queue_high_watermark {
            self.high_water_warning_active
                .store(false, Ordering::Release);
        }
    }

    pub(crate) fn stats(&self) -> ObserverQueueStats {
        ObserverQueueStats {
            depth: self.queue_depth.load(Ordering::Acquire),
            capacity: self.queue_capacity,
            high_watermark: self.queue_high_watermark,
            high_water_warning_count: self.high_water_warning_count.load(Ordering::Relaxed),
            cap_breach_count: self.cap_breach_count.load(Ordering::Relaxed),
            poisoned: self.poisoned.load(Ordering::Acquire),
        }
    }

    pub(crate) async fn wait_drained(&self) {
        loop {
            if self.drained.load(Ordering::Acquire) {
                return;
            }
            let notified = self.drained_notify.notified();
            if self.drained.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn take_receiver(
        &self,
    ) -> mpsc::UnboundedReceiver<CommittedMutationObserverMessage> {
        assert!(
            !self.started.swap(true, Ordering::AcqRel),
            "tenant observer dispatcher must be started exactly once"
        );
        self.receiver
            .lock()
            .expect("observer receiver lock should not be poisoned")
            .take()
            .expect("observer receiver should exist before task start")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ObserverQueueStats {
    pub(crate) depth: usize,
    pub(crate) capacity: usize,
    pub(crate) high_watermark: usize,
    pub(crate) high_water_warning_count: u64,
    pub(crate) cap_breach_count: u64,
    pub(crate) poisoned: bool,
}

pub(crate) struct PendingPublisherResponse {
    pub(crate) _operation: TenantOperationGuard,
    pub(crate) response: MutationResponseSender,
    pub(crate) result: QueuedMutationResult,
}

pub(crate) struct DeferredPublisherResponse {
    pub(crate) _operation: TenantOperationGuard,
    pub(crate) response: MutationResponseSender,
    pub(crate) result: Result<QueuedMutationResult>,
}

impl DeferredPublisherResponse {
    pub(crate) fn complete(self) {
        let _ = self.response.send(self.result);
    }

    pub(crate) fn complete_after_recovery(
        self,
        discarded_first_sequence: SequenceNumber,
        recovery_error: &Error,
    ) {
        let discarded_wait_target = self
            .result
            .as_ref()
            .err()
            .and_then(Error::conflicting_sequence)
            .filter(|sequence| *sequence >= discarded_first_sequence);
        if let Some(sequence) = discarded_wait_target {
            self.fail(&Error::rejected_before_execution(format!(
                "publisher recovery discarded conflict wait target {sequence} at or after {discarded_first_sequence}: {recovery_error}"
            )));
        } else {
            self.complete();
        }
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

    pub(crate) fn defer_failure(self, error: &Error) -> Vec<Box<dyn FnOnce() + Send + 'static>> {
        self.responses
            .into_iter()
            .map(|pending| {
                let PendingPublisherResponse {
                    _operation,
                    response,
                    ..
                } = pending;
                drop(_operation);
                let error = error.clone();
                Box::new(move || {
                    let _ = response.send(Err(error));
                }) as Box<dyn FnOnce() + Send + 'static>
            })
            .collect()
    }

    pub(crate) fn try_merge(&mut self, mut next: Self) -> std::result::Result<(), Box<Self>> {
        if !Arc::ptr_eq(&self.engine, &next.engine)
            || self.last_sequence().0.checked_add(1) != Some(next.first_sequence().0)
        {
            return Err(Box::new(next));
        }
        Arc::make_mut(&mut self.records).extend(next.records.iter().cloned());
        self.responses.append(&mut next.responses);
        self.phases.merge_assignment(next.phases);
        self.sample_started_at = self.sample_started_at.min(next.sample_started_at);
        Ok(())
    }
}

impl DeferredPublisherResponse {
    pub(crate) fn defer_failure(self, error: &Error) -> Box<dyn FnOnce() + Send + 'static> {
        let Self {
            _operation,
            response,
            ..
        } = self;
        drop(_operation);
        let error = error.clone();
        Box::new(move || {
            let _ = response.send(Err(error));
        })
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
    mode_transition_failure_count: AtomicU64,
    requested_mode_override: AtomicU8,
    assignment_recovery_gate: tokio::sync::Mutex<()>,
}

impl PublisherHandoff {
    pub(crate) fn new(pipeline_capable: bool, _tenant_id: &TenantId) -> Self {
        #[cfg(test)]
        let (capacity, send_timeout) =
            take_publisher_limits_for_testing(_tenant_id).unwrap_or_else(publisher_limits_from_env);
        #[cfg(not(test))]
        let (capacity, send_timeout) = publisher_limits_from_env();
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
            mode_transition_failure_count: AtomicU64::new(0),
            requested_mode_override: AtomicU8::new(0),
            assignment_recovery_gate: tokio::sync::Mutex::new(()),
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
                    self.mode_transition_failure_count
                        .fetch_add(1, Ordering::Relaxed);
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
                    self.mode_transition_failure_count
                        .fetch_add(1, Ordering::Relaxed);
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

    pub(crate) fn pipeline_capable(&self) -> bool {
        self.pipeline_capable
    }

    pub(crate) async fn lock_assignment_recovery(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.assignment_recovery_gate.lock().await
    }

    pub(crate) fn mode_transition_count(&self) -> u64 {
        self.mode_transition_count.load(Ordering::Relaxed)
    }

    pub(crate) fn mode_transition_failure_count(&self) -> u64 {
        self.mode_transition_failure_count.load(Ordering::Relaxed)
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
        match self.reserve("assigned batch").await {
            Ok(permit) => {
                permit.send(PublisherMessage::Batch(batch));
                Ok(())
            }
            Err(error) => Err(Box::new((batch, error))),
        }
    }

    pub(crate) async fn barrier(&self) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        let permit = self.reserve("serial barrier").await?;
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
        match self.reserve("response fence").await {
            Ok(permit) => {
                permit.send(PublisherMessage::ResponseFence(responses));
                Ok(())
            }
            Err(error) => Err(Box::new((responses, error))),
        }
    }

    pub(crate) async fn send_serial_job(
        &self,
        job: CommitterJob,
    ) -> std::result::Result<oneshot::Receiver<()>, (CommitterJob, Error)> {
        match self.reserve("serial job").await {
            Ok(permit) => {
                let (drained, wait_for_drain) = oneshot::channel();
                permit.send(PublisherMessage::SerialJob { job, drained });
                Ok(wait_for_drain)
            }
            Err(error) => Err((job, error)),
        }
    }

    async fn reserve(&self, operation: &'static str) -> Result<mpsc::Permit<'_, PublisherMessage>> {
        match tokio::time::timeout(self.send_timeout, self.sender.reserve()).await {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => Err(Error::Internal(format!(
                "tenant publisher stopped before accepting {operation}"
            ))),
            Err(_) => {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Err(Error::committer_full(
                    format!(
                        "tenant publisher {operation} queue remained full for {} ms (capacity {})",
                        self.send_timeout.as_millis(),
                        self.capacity
                    ),
                    self.capacity,
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
