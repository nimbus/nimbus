use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord, TenantId};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::Engine;
use crate::engine::CommitPhaseDurations;
use crate::engine::committed_mutations::{
    CommittedMutationObserverDispatch, CommittedMutationObserverMessage, ProjectionToken,
};

use super::super::{
    CommitterJob, MutationResponseSender, QueuedMutationResult, TenantOperationGuard,
};
use super::CommitterArm;
#[cfg(any(test, feature = "test-hooks"))]
use crate::tenant::pause_barrier::{PauseBarrier, PauseBarrierHandle};

const DEFAULT_PUBLISHER_QUEUE_CAPACITY: usize = 32;
const DEFAULT_PUBLISHER_SEND_TIMEOUT_MS: u64 = 500;
const DEFAULT_OBSERVER_QUEUE_CAPACITY: usize = 4_096;
const DEFAULT_OBSERVER_QUEUE_HIGH_WATERMARK: usize = 3_072;
const OBSERVER_DRAIN_BLOCKING_TIMEOUT: Duration = Duration::from_secs(30);

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

fn observer_limits_from_env() -> (usize, usize, usize, usize) {
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
        crate::config::mutation_journal_batch_max(),
    )
}

fn clamp_observer_limits(
    tenant_id: &TenantId,
    requested_capacity: usize,
    requested_high_watermark: usize,
    publisher_max_dispatch_size: usize,
    journal_max_dispatch_size: usize,
) -> (usize, usize, usize) {
    let max_dispatch_size = publisher_max_dispatch_size.max(journal_max_dispatch_size);
    let minimum_capacity = max_dispatch_size.max(1).saturating_add(1);
    let capacity = requested_capacity.max(1).max(minimum_capacity);
    if capacity != requested_capacity {
        tracing::warn!(
            tenant = %tenant_id,
            requested_observer_queue_capacity = requested_capacity,
            max_publisher_observer_dispatch_size = publisher_max_dispatch_size,
            max_serial_observer_dispatch_size = journal_max_dispatch_size,
            observer_queue_capacity = capacity,
            "clamped committed mutation observer capacity to the maximum live dispatch plus catch-up headroom"
        );
    }
    let high_watermark = requested_high_watermark.max(1).min(capacity);
    (capacity, high_watermark, max_dispatch_size)
}

#[cfg(test)]
static PUBLISHER_LIMITS_FOR_TESTING: std::sync::OnceLock<
    Mutex<std::collections::HashMap<TenantId, (usize, Duration)>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static COMMITTER_ARMS_FOR_TESTING: std::sync::OnceLock<
    Mutex<std::collections::HashMap<TenantId, CommitterArm>>,
> = std::sync::OnceLock::new();

/// Selects a tenant's committer arm before its runtime is constructed.
///
/// The selection is consumed exactly once by [`PublisherHandoff::new`]. Tests
/// use this seam to exercise both static adapters without mutating a live
/// runtime; production derives the same immutable choice from persistence
/// topology.
#[cfg(test)]
pub(crate) fn configure_committer_arm_for_testing(tenant_id: TenantId, arm: CommitterArm) {
    COMMITTER_ARMS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("committer-arm test-selection lock should not be poisoned")
        .insert(tenant_id, arm);
}

#[cfg(test)]
fn take_committer_arm_for_testing(tenant_id: &TenantId) -> Option<CommitterArm> {
    COMMITTER_ARMS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("committer-arm test-selection lock should not be poisoned")
        .remove(tenant_id)
}

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
type ObserverLimitsForTesting = (usize, usize, usize, usize);

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
    publisher_max_dispatch_size: usize,
    journal_max_dispatch_size: usize,
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
                publisher_max_dispatch_size.max(1),
                journal_max_dispatch_size.max(1),
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

#[cfg(test)]
static OBSERVER_DRAIN_TIMEOUTS_FOR_TESTING: std::sync::OnceLock<
    Mutex<std::collections::HashMap<TenantId, Duration>>,
> = std::sync::OnceLock::new();

/// Shortens the blocking observer-drain deadline so a test can reach the
/// timeout branch without parking for the production 30-second bound.
#[cfg(test)]
pub(crate) fn configure_observer_drain_blocking_timeout_for_testing(
    tenant_id: TenantId,
    drain_timeout: Duration,
) {
    OBSERVER_DRAIN_TIMEOUTS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("observer test-drain-timeout lock should not be poisoned")
        .insert(tenant_id, drain_timeout);
}

#[cfg(test)]
fn take_observer_drain_blocking_timeout_for_testing(tenant_id: &TenantId) -> Option<Duration> {
    OBSERVER_DRAIN_TIMEOUTS_FOR_TESTING
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("observer test-drain-timeout lock should not be poisoned")
        .remove(tenant_id)
}

/// Slice-A overload contract: live observer delivery is bounded and loud.
/// A cap breach poisons this tenant handoff and records diagnostics before an
/// error is logged. Blocking the publisher here can deadlock because observers
/// may perform nested synchronous writes; leaving the queue truly unbounded can
/// exhaust process memory. Lossless catch-up is the PPSC5-B durable-journal
/// replay contract, not an implicit property of this live handoff.
pub(crate) struct ObserverHandoff {
    tenant_id: TenantId,
    sender: Mutex<ObserverSender>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<CommittedMutationObserverMessage>>>,
    queue_depth: AtomicUsize,
    queue_peak_depth: AtomicUsize,
    queue_capacity: usize,
    queue_high_watermark: usize,
    max_live_dispatch: usize,
    high_water_warning_active: AtomicBool,
    high_water_warning_count: AtomicU64,
    cap_breach_count: AtomicU64,
    catch_up_enqueue_failure_count: AtomicU64,
    poisoned: AtomicBool,
    started: AtomicBool,
    drained: AtomicBool,
    drain_blocking_timeout: Duration,
    drained_notify: tokio::sync::Notify,
    capacity_available: tokio::sync::Notify,
    catch_up_pending: AtomicBool,
    catch_up_next_sequence: AtomicU64,
    catch_up_requested_through: AtomicU64,
    catch_up_projection_token: Mutex<ProjectionToken>,
    catch_up_state_changed: tokio::sync::Notify,
    #[cfg(test)]
    catch_up_task_count: AtomicUsize,
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
        #[cfg(test)]
        let drain_blocking_timeout = take_observer_drain_blocking_timeout_for_testing(tenant_id)
            .unwrap_or(OBSERVER_DRAIN_BLOCKING_TIMEOUT);
        #[cfg(not(test))]
        let drain_blocking_timeout = OBSERVER_DRAIN_BLOCKING_TIMEOUT;
        let (queue_capacity, queue_high_watermark, max_live_dispatch) = clamp_observer_limits(
            tenant_id,
            requested_limits.0,
            requested_limits.1,
            requested_limits.2,
            requested_limits.3,
        );
        Self {
            tenant_id: tenant_id.clone(),
            sender: Mutex::new(ObserverSender {
                sender,
                closed: false,
            }),
            receiver: Mutex::new(Some(receiver)),
            queue_depth: AtomicUsize::new(0),
            queue_peak_depth: AtomicUsize::new(0),
            queue_capacity,
            queue_high_watermark,
            max_live_dispatch,
            high_water_warning_active: AtomicBool::new(false),
            high_water_warning_count: AtomicU64::new(0),
            cap_breach_count: AtomicU64::new(0),
            catch_up_enqueue_failure_count: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            started: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            drain_blocking_timeout,
            drained_notify: tokio::sync::Notify::new(),
            capacity_available: tokio::sync::Notify::new(),
            catch_up_pending: AtomicBool::new(false),
            catch_up_next_sequence: AtomicU64::new(u64::MAX),
            catch_up_requested_through: AtomicU64::new(0),
            catch_up_projection_token: Mutex::new(ProjectionToken::default()),
            catch_up_state_changed: tokio::sync::Notify::new(),
            #[cfg(test)]
            catch_up_task_count: AtomicUsize::new(0),
        }
    }

    pub(crate) fn send(
        &self,
        dispatch: CommittedMutationObserverDispatch,
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

        self.send_locked(dispatch, runtime, &mut sender, depth)
    }

    pub(crate) async fn send_when_capacity_available(
        &self,
        dispatch: CommittedMutationObserverDispatch,
        runtime: std::sync::Weak<super::super::TenantRuntime>,
    ) -> Result<()> {
        let event_count = dispatch.event_count();
        let catch_up_capacity = self.queue_capacity.saturating_sub(self.max_live_dispatch);
        if event_count > catch_up_capacity {
            return Err(Error::Internal(format!(
                "committed mutation observer catch-up dispatch contains {event_count} events, exceeding tenant {} catch-up allowance {catch_up_capacity}",
                self.tenant_id
            )));
        }
        loop {
            let available = self.capacity_available.notified();
            {
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
                        "observer catch-up dispatch arrived after the ordered dispatcher close",
                    );
                    return Err(self.poisoned_error());
                }
                let depth = self.queue_depth.load(Ordering::Acquire);
                if event_count.saturating_add(self.max_live_dispatch)
                    <= self.queue_capacity.saturating_sub(depth)
                {
                    return self.send_locked(dispatch, runtime, &mut sender, depth);
                }
            }
            available.await;
        }
    }

    fn send_locked(
        &self,
        mut dispatch: CommittedMutationObserverDispatch,
        runtime: std::sync::Weak<super::super::TenantRuntime>,
        sender: &mut ObserverSender,
        depth: usize,
    ) -> Result<()> {
        let event_count = dispatch.event_count();
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
        // Publish the peak before handing the dispatch to the receiver. Tests
        // and diagnostics awakened by the callback must not race this update.
        self.queue_peak_depth
            .fetch_max(next_depth, Ordering::Release);
        if let Err(rejected) = sender
            .sender
            .send(CommittedMutationObserverMessage::Dispatch(dispatch))
        {
            self.poison_locked(
                sender,
                "committed mutation observer dispatcher stopped while accepting a dispatch",
            );
            let CommittedMutationObserverMessage::Dispatch(mut dispatch) = rejected.0 else {
                unreachable!("observer send failure must return the dispatched message")
            };
            dispatch.disarm_completion();
            self.complete_dispatch_locked(event_count);
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
        self.capacity_available.notify_waiters();
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
        self.capacity_available.notify_waiters();
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
        self.capacity_available.notify_waiters();
    }

    pub(crate) fn stats(&self) -> ObserverQueueStats {
        ObserverQueueStats {
            depth: self.queue_depth.load(Ordering::Acquire),
            peak_depth: self.queue_peak_depth.load(Ordering::Acquire),
            capacity: self.queue_capacity,
            high_watermark: self.queue_high_watermark,
            high_water_warning_count: self.high_water_warning_count.load(Ordering::Relaxed),
            cap_breach_count: self.cap_breach_count.load(Ordering::Relaxed),
            catch_up_enqueue_failure_count: self
                .catch_up_enqueue_failure_count
                .load(Ordering::Relaxed),
            poisoned: self.poisoned.load(Ordering::Acquire),
        }
    }

    pub(crate) fn catch_up_chunk_size(&self) -> usize {
        let catch_up_capacity = self
            .queue_capacity
            .saturating_sub(self.max_live_dispatch)
            .max(1);
        (self.queue_capacity / 2).max(1).min(catch_up_capacity)
    }

    pub(crate) fn request_catch_up(
        &self,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: ProjectionToken,
    ) -> bool {
        // Publish provenance and then the upper bound before making a start
        // visible. A taker can safely observe a newer token than its claimed
        // range, but must never pair a newer range with older provenance.
        {
            let mut pending_token = self
                .catch_up_projection_token
                .lock()
                .expect("observer catch-up projection-token lock should not be poisoned");
            *pending_token = (*pending_token).max(projection_token);
        }
        self.catch_up_requested_through
            .fetch_max(requested_through.0, Ordering::AcqRel);
        self.catch_up_next_sequence
            .fetch_min(first_sequence.0, Ordering::AcqRel);
        self.catch_up_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn take_catch_up_request(
        &self,
    ) -> Option<(SequenceNumber, SequenceNumber, ProjectionToken)> {
        let first_sequence = self.catch_up_next_sequence.swap(u64::MAX, Ordering::AcqRel);
        (first_sequence != u64::MAX).then(|| {
            (
                SequenceNumber(first_sequence),
                SequenceNumber(self.catch_up_requested_through.load(Ordering::Acquire)),
                *self
                    .catch_up_projection_token
                    .lock()
                    .expect("observer catch-up projection-token lock should not be poisoned"),
            )
        })
    }

    pub(crate) fn complete_catch_up(&self) -> bool {
        if self.catch_up_next_sequence.load(Ordering::Acquire) != u64::MAX {
            return true;
        }
        self.catch_up_pending.store(false, Ordering::Release);
        self.catch_up_state_changed.notify_waiters();
        if self.catch_up_next_sequence.load(Ordering::Acquire) == u64::MAX {
            return false;
        }
        self.catch_up_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn abandon_catch_up(
        &self,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: ProjectionToken,
    ) {
        // Match request_catch_up's publication order for a returned request.
        {
            let mut pending_token = self
                .catch_up_projection_token
                .lock()
                .expect("observer catch-up projection-token lock should not be poisoned");
            *pending_token = (*pending_token).max(projection_token);
        }
        self.catch_up_requested_through
            .fetch_max(requested_through.0, Ordering::AcqRel);
        self.catch_up_next_sequence
            .fetch_min(first_sequence.0, Ordering::AcqRel);
        self.catch_up_pending.store(false, Ordering::Release);
        self.catch_up_state_changed.notify_waiters();
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_catch_up_idle(&self) {
        loop {
            let changed = self.catch_up_state_changed.notified();
            if !self.catch_up_pending.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }

    #[cfg(test)]
    pub(crate) fn record_catch_up_task_started(&self) {
        self.catch_up_task_count.fetch_add(1, Ordering::AcqRel);
    }

    #[cfg(test)]
    pub(crate) fn record_catch_up_task_finished(&self) {
        let previous = self.catch_up_task_count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(
            previous != 0,
            "observer catch-up task count cannot underflow"
        );
    }

    #[cfg(test)]
    pub(crate) fn catch_up_task_count(&self) -> usize {
        self.catch_up_task_count.load(Ordering::Acquire)
    }

    pub(crate) fn record_catch_up_enqueue_failure(&self) {
        self.catch_up_enqueue_failure_count
            .fetch_add(1, Ordering::Relaxed);
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

    pub(crate) async fn wait_drained_for_eviction(&self) -> Result<()> {
        if tokio::time::timeout(self.drain_blocking_timeout, self.wait_drained())
            .await
            .is_ok()
        {
            return Ok(());
        }
        let stats = self.stats();
        tracing::error!(
            tenant = %self.tenant_id,
            timeout = ?self.drain_blocking_timeout,
            observer_queue_depth = stats.depth,
            observer_queue_capacity = stats.capacity,
            observer_dispatcher_poisoned = stats.poisoned,
            "timed out waiting for committed-mutation observer dispatcher to drain during durable-recovery eviction"
        );
        Err(Error::Internal(format!(
            "committed-mutation observer dispatcher for tenant {} did not drain within {:?}",
            self.tenant_id, self.drain_blocking_timeout
        )))
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
    pub(crate) peak_depth: usize,
    pub(crate) capacity: usize,
    pub(crate) high_watermark: usize,
    pub(crate) high_water_warning_count: u64,
    pub(crate) cap_breach_count: u64,
    pub(crate) catch_up_enqueue_failure_count: u64,
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

    pub(crate) fn defer_completion_after_recovery(
        self,
        discarded_first_sequence: SequenceNumber,
        recovery_error: &Error,
    ) -> Box<dyn FnOnce() + Send + 'static> {
        let Self {
            _operation,
            response,
            result,
        } = self;
        let discarded_wait_target = result
            .as_ref()
            .err()
            .and_then(Error::conflicting_sequence)
            .filter(|sequence| *sequence >= discarded_first_sequence);
        let result = if let Some(sequence) = discarded_wait_target {
            Err(Error::rejected_before_execution(format!(
                "publisher recovery discarded conflict wait target {sequence} at or after {discarded_first_sequence}: {recovery_error}"
            )))
        } else {
            result
        };
        drop(_operation);
        Box::new(move || {
            let _ = response.send(result);
        })
    }
}

pub(crate) struct AssignedPublisherBatch {
    pub(crate) engine: Arc<Engine>,
    pub(crate) records: Arc<Vec<TenantEventRecord>>,
    pub(crate) responses: Vec<PendingPublisherResponse>,
    pub(crate) deferred: Vec<DeferredPublisherResponse>,
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

    pub(crate) fn fail_after_recovery(
        self,
        discarded_first_sequence: SequenceNumber,
        recovery_error: &Error,
    ) {
        for pending in self.responses {
            let _ = pending.response.send(Err(recovery_error.clone()));
        }
        for deferred in self.deferred {
            deferred.complete_after_recovery(discarded_first_sequence, recovery_error);
        }
    }

    pub(crate) fn defer_failure_after_recovery(
        self,
        discarded_first_sequence: SequenceNumber,
        error: &Error,
    ) -> Vec<Box<dyn FnOnce() + Send + 'static>> {
        let mut completions = self
            .responses
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
            .collect::<Vec<_>>();
        completions.extend(self.deferred.into_iter().map(|deferred| {
            deferred.defer_completion_after_recovery(discarded_first_sequence, error)
        }));
        completions
    }

    pub(crate) fn try_merge(&mut self, mut next: Self) -> std::result::Result<(), Box<Self>> {
        if !Arc::ptr_eq(&self.engine, &next.engine)
            || self.last_sequence().0.checked_add(1) != Some(next.first_sequence().0)
        {
            return Err(Box::new(next));
        }
        Arc::make_mut(&mut self.records).extend(next.records.iter().cloned());
        self.responses.append(&mut next.responses);
        self.deferred.append(&mut next.deferred);
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

pub(crate) struct PublisherResponseFenceError {
    responses: Vec<DeferredPublisherResponse>,
    error: Error,
    queue_closed: bool,
}

impl PublisherResponseFenceError {
    pub(crate) fn into_parts(self) -> (Vec<DeferredPublisherResponse>, Error, bool) {
        (self.responses, self.error, self.queue_closed)
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct PublisherErrorCounts {
    pub(crate) transient: u64,
    pub(crate) fatal: u64,
    pub(crate) ambiguous: u64,
}

pub(crate) enum PublisherMessage {
    Batch(AssignedPublisherBatch),
    ResponseFence(Vec<DeferredPublisherResponse>),
    OrderedOpaqueJob {
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
    arm: CommitterArm,
    assignment_recovery_gate: tokio::sync::Mutex<()>,
    finished: AtomicBool,
    finished_notify: Notify,
    #[cfg(any(test, feature = "test-hooks"))]
    pause_before_message: Arc<PauseBarrier>,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub struct OrderedPublisherPauseHandle(PauseBarrierHandle);

#[cfg(any(test, feature = "test-hooks"))]
impl OrderedPublisherPauseHandle {
    pub fn arm(&self) {
        self.0.arm();
    }

    pub fn wait_until_entered(&self, timeout: Duration) -> bool {
        self.0.wait_until_entered(timeout).is_some()
    }

    pub fn release(&self) {
        self.0.release();
    }
}

impl PublisherHandoff {
    pub(crate) fn new(committer_arm: CommitterArm, _tenant_id: &TenantId) -> Self {
        #[cfg(test)]
        let (capacity, send_timeout) =
            take_publisher_limits_for_testing(_tenant_id).unwrap_or_else(publisher_limits_from_env);
        #[cfg(not(test))]
        let (capacity, send_timeout) = publisher_limits_from_env();
        let (sender, receiver) = mpsc::channel(capacity);
        #[cfg(test)]
        let committer_arm = take_committer_arm_for_testing(_tenant_id).unwrap_or(committer_arm);
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
            arm: committer_arm,
            assignment_recovery_gate: tokio::sync::Mutex::new(()),
            finished: AtomicBool::new(false),
            finished_notify: Notify::new(),
            #[cfg(any(test, feature = "test-hooks"))]
            pause_before_message: Arc::new(PauseBarrier::default()),
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn pause_handle(&self) -> OrderedPublisherPauseHandle {
        OrderedPublisherPauseHandle(PauseBarrierHandle::new(self.pause_before_message.clone()))
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_for_test_pause(&self) {
        let pause = self.pause_before_message.clone();
        tokio::task::spawn_blocking(move || pause.wait_if_armed(()))
            .await
            .expect("ordered publisher pause task should not panic");
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn release_test_pause_for_shutdown(&self) {
        self.pause_before_message.release_for_shutdown();
    }

    pub(crate) fn uses_ordered_publisher(&self) -> bool {
        self.arm.uses_ordered_publisher()
    }

    pub(crate) fn arm(&self) -> CommitterArm {
        self.arm
    }

    pub(crate) async fn lock_assignment_recovery(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.assignment_recovery_gate.lock().await
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

    pub(crate) async fn send_response_fence(
        &self,
        responses: Vec<DeferredPublisherResponse>,
    ) -> std::result::Result<(), Box<PublisherResponseFenceError>> {
        if responses.is_empty() {
            return Ok(());
        }
        match self.reserve("response fence").await {
            Ok(permit) => {
                permit.send(PublisherMessage::ResponseFence(responses));
                Ok(())
            }
            Err(error) => Err(Box::new(PublisherResponseFenceError {
                responses,
                error,
                queue_closed: self.sender.is_closed(),
            })),
        }
    }

    pub(crate) fn mark_finished(&self) {
        self.finished.store(true, Ordering::Release);
        self.finished_notify.notify_waiters();
    }

    pub(crate) async fn wait_finished(&self) {
        loop {
            let notified = self.finished_notify.notified();
            if self.finished.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    pub(crate) async fn send_ordered_opaque_job(
        &self,
        job: CommitterJob,
    ) -> std::result::Result<oneshot::Receiver<()>, (CommitterJob, Error)> {
        match self.reserve("ordered opaque job").await {
            Ok(permit) => {
                let (drained, wait_for_drain) = oneshot::channel();
                permit.send(PublisherMessage::OrderedOpaqueJob { job, drained });
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
