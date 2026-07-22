use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::OnceLock;

use nimbus_core::{Error, Result, SequenceNumber, TenantId};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::Engine;
use crate::engine::{TenantEvictionRegistry, begin_definitive_fence_eviction};

use super::super::TenantRuntime;

const DEFAULT_COMMITTER_INBOX_SIZE: usize = 128;
const DEFAULT_COMMITTER_SEND_TIMEOUT_MS: u64 = 500;

fn committer_limits_from_env() -> (usize, Duration) {
    (
        crate::config::env_positive_usize(
            "NIMBUS_COMMITTER_INBOX_SIZE",
            DEFAULT_COMMITTER_INBOX_SIZE,
        ),
        Duration::from_millis(crate::config::env_nonnegative_u64(
            "NIMBUS_COMMITTER_SEND_TIMEOUT_MS",
            DEFAULT_COMMITTER_SEND_TIMEOUT_MS,
        )),
    )
}

#[cfg(test)]
static COMMITTER_LIMITS_FOR_TESTING: OnceLock<Mutex<HashMap<TenantId, (usize, Duration)>>> =
    OnceLock::new();

#[cfg(test)]
pub(crate) fn configure_committer_limits_for_testing(
    tenant_id: TenantId,
    capacity: usize,
    send_timeout: Duration,
) {
    COMMITTER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("committer test-limit lock should not be poisoned")
        .insert(tenant_id, (capacity.max(1), send_timeout));
}

#[cfg(test)]
fn take_committer_limits_for_testing(tenant_id: &TenantId) -> Option<(usize, Duration)> {
    COMMITTER_LIMITS_FOR_TESTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("committer test-limit lock should not be poisoned")
        .remove(tenant_id)
}

tokio::task_local! {
    static COMMITTER_ACTOR_ACTIVE: TenantId;
}

thread_local! {
    static COMMITTER_HANDLER_ACTIVE: std::cell::RefCell<Option<TenantId>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct CommitterJob {
    task: Box<dyn FnOnce() + Send + 'static>,
    rejected: Box<dyn FnOnce(Error) + Send + 'static>,
    completed: oneshot::Sender<()>,
}

impl CommitterJob {
    pub(crate) fn new(
        task: impl FnOnce() + Send + 'static,
        rejected: impl FnOnce(Error) + Send + 'static,
    ) -> (Self, oneshot::Receiver<()>) {
        let (completed, completion) = oneshot::channel();
        (
            Self {
                task: Box::new(task),
                rejected: Box::new(rejected),
                completed,
            },
            completion,
        )
    }

    pub(crate) fn into_parts(self) -> (Box<dyn FnOnce() + Send + 'static>, oneshot::Sender<()>) {
        (self.task, self.completed)
    }

    pub(crate) fn fail(self, error: Error) {
        (self.rejected)(error);
        let _ = self.completed.send(());
    }

    pub(crate) fn defer_failure(self, error: Error) -> Box<dyn FnOnce() + Send + 'static> {
        let Self {
            task,
            rejected,
            completed,
        } = self;
        drop(task);
        Box::new(move || {
            rejected(error);
            let _ = completed.send(());
        })
    }
}

/// Pure serial assignment boundary. It accepts only plain sequence data,
/// performs no storage access, and cannot await. Validation that may consult
/// prepared state happens before this call; durable append/apply happens after.
pub(crate) fn assign_and_validate(
    previous: SequenceNumber,
    count: usize,
) -> Result<Vec<SequenceNumber>> {
    (1..=count)
        .map(|offset| {
            let offset = u64::try_from(offset)
                .map_err(|_| Error::Internal("commit batch size exceeds u64".to_string()))?;
            previous
                .0
                .checked_add(offset)
                .map(SequenceNumber)
                .ok_or_else(|| Error::Internal("tenant commit sequence exhausted".to_string()))
        })
        .collect()
}

/// Cheap always-on guard at the append boundary. An actor may assign only the
/// exact contiguous suffix following the tenant's prior durable sequence.
pub(crate) fn validate_append_sequences(
    previous: SequenceNumber,
    appended: impl IntoIterator<Item = SequenceNumber>,
) -> Result<()> {
    let mut expected = previous;
    for actual in appended {
        expected = SequenceNumber(
            expected
                .0
                .checked_add(1)
                .ok_or_else(|| Error::Internal("tenant commit sequence exhausted".to_string()))?,
        );
        if actual != expected {
            return Err(Error::Internal(format!(
                "committer append sequence invariant violated: expected {expected} after {previous}, got {actual}"
            )));
        }
    }
    Ok(())
}

pub(crate) enum CommitterMessage {
    QueuedBatch {
        engine: Arc<Engine>,
        owns_pending_wake: bool,
    },
    DirectCommit(CommitterJob),
    ExecutionUnitCommit(CommitterJob),
    JournalProgressSync(CommitterJob),
    InternalCommit(CommitterJob),
}

impl CommitterMessage {
    fn kind(&self) -> &'static str {
        match self {
            Self::QueuedBatch { .. } => "queued-batch",
            Self::DirectCommit(_) => "direct",
            Self::ExecutionUnitCommit(_) => "execution-unit",
            Self::JournalProgressSync(_) => "journal-progress",
            Self::InternalCommit(_) => "internal",
        }
    }
}

pub(crate) struct CommitterActor {
    tenant_id: TenantId,
    sender: mpsc::Sender<CommitterMessage>,
    receiver: Mutex<Option<mpsc::Receiver<CommitterMessage>>>,
    started: AtomicBool,
    queued_batch_pending: AtomicBool,
    inbox_capacity: usize,
    send_timeout: Duration,
    send_timeout_count: AtomicU64,
    shutdown: CancellationToken,
}

impl CommitterActor {
    pub(crate) fn new(tenant_id: TenantId) -> Self {
        #[cfg(test)]
        let (inbox_capacity, send_timeout) =
            take_committer_limits_for_testing(&tenant_id).unwrap_or_else(committer_limits_from_env);
        #[cfg(not(test))]
        let (inbox_capacity, send_timeout) = committer_limits_from_env();
        let (sender, receiver) = mpsc::channel(inbox_capacity);
        Self {
            tenant_id,
            sender,
            receiver: Mutex::new(Some(receiver)),
            started: AtomicBool::new(false),
            queued_batch_pending: AtomicBool::new(false),
            inbox_capacity,
            send_timeout,
            send_timeout_count: AtomicU64::new(0),
            shutdown: CancellationToken::new(),
        }
    }

    pub(crate) fn take_receiver(&self) -> mpsc::Receiver<CommitterMessage> {
        assert!(
            !self.started.swap(true, Ordering::AcqRel),
            "tenant committer actor must be started exactly once"
        );
        self.receiver
            .lock()
            .expect("committer receiver lock should not be poisoned")
            .take()
            .expect("tenant committer receiver should exist before actor start")
    }

    pub(crate) async fn send_async(&self, message: CommitterMessage) -> Result<()> {
        assert_not_reentrant(&self.tenant_id);
        let timeout = self.send_timeout;
        match tokio::time::timeout(timeout, self.sender.send(message)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(Error::Internal(format!(
                "tenant committer actor stopped before accepting {} work",
                error.0.kind()
            ))),
            Err(_) => {
                self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                Err(Error::committer_full(
                    format!(
                        "tenant committer inbox remained full for {} ms (capacity {})",
                        timeout.as_millis(),
                        self.inbox_capacity
                    ),
                    self.inbox_capacity,
                ))
            }
        }
    }

    pub(crate) async fn send_queued_batch_async(&self, engine: Arc<Engine>) -> Result<()> {
        assert_not_reentrant(&self.tenant_id);
        if self.queued_batch_pending.load(Ordering::Acquire) {
            return Ok(());
        }
        match self.sender.try_reserve() {
            Ok(permit) => {
                if self
                    .queued_batch_pending
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    permit.send(CommitterMessage::QueuedBatch {
                        engine,
                        owns_pending_wake: true,
                    });
                }
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(())) => Err(Error::Internal(
                "tenant committer actor stopped before accepting queued-batch work".to_string(),
            )),
            Err(mpsc::error::TrySendError::Full(())) => {
                // Coalesce only once a channel slot is already reserved. If
                // the inbox is full, this request retains its own bounded send
                // and timeout; no follower can trust a wake that may still
                // fail to enter the channel.
                self.send_async(CommitterMessage::QueuedBatch {
                    engine,
                    owns_pending_wake: false,
                })
                .await
            }
        }
    }

    pub(crate) fn accept_queued_batch(&self, owns_pending_wake: bool) {
        if owns_pending_wake {
            let pending = self.queued_batch_pending.swap(false, Ordering::AcqRel);
            debug_assert!(pending, "a tracked queued-batch wake must be pending");
        }
    }

    pub(crate) fn send_blocking(&self, mut message: CommitterMessage) -> Result<()> {
        assert_not_reentrant(&self.tenant_id);
        let timeout = self.send_timeout;
        let deadline = Instant::now() + timeout;
        loop {
            match self.sender.try_send(message) {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TrySendError::Closed(returned)) => {
                    return Err(Error::Internal(format!(
                        "tenant committer actor stopped before accepting {} work",
                        returned.kind()
                    )));
                }
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    message = returned;
                    let now = Instant::now();
                    if now >= deadline {
                        self.send_timeout_count.fetch_add(1, Ordering::Relaxed);
                        return Err(Error::committer_full(
                            format!(
                                "tenant committer inbox remained full for {} ms (capacity {})",
                                timeout.as_millis(),
                                self.inbox_capacity
                            ),
                            self.inbox_capacity,
                        ));
                    }
                    std::thread::park_timeout(
                        deadline
                            .saturating_duration_since(now)
                            .min(Duration::from_millis(1)),
                    );
                }
            }
        }
    }

    pub(crate) fn submit_blocking<T, F>(
        &self,
        wrap: impl FnOnce(CommitterJob) -> CommitterMessage,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        if on_actor_task(&self.tenant_id) {
            return task();
        }
        let (response_tx, response_rx) = oneshot::channel();
        let response = Arc::new(Mutex::new(Some(response_tx)));
        let task_response = response.clone();
        let rejected_response = response;
        let (job, completion) = CommitterJob::new(
            move || {
                if let Some(response) = task_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                {
                    let _ = response.send(task());
                }
            },
            move |error| {
                if let Some(response) = rejected_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                {
                    let _ = response.send(Err(error));
                }
            },
        );
        self.send_blocking(wrap(job))?;
        let result = blocking_receive(
            response_rx,
            "tenant committer actor dropped a blocking response",
        );
        let completion = blocking_receive(
            completion,
            "tenant committer actor dropped a blocking completion",
        );
        completion?;
        result?
    }

    /// Runs `after_commit` on the caller before the actor accepts its next
    /// message. This is used only for non-reentrant subscription/trigger
    /// enqueueing; arbitrary observer callbacks run after this boundary.
    pub(crate) fn submit_blocking_then<T, F, A>(
        &self,
        wrap: impl FnOnce(CommitterJob) -> CommitterMessage,
        task: F,
        after_commit: A,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
        A: FnOnce(&T),
    {
        if on_actor_task(&self.tenant_id) {
            let result = task();
            if let Ok(value) = &result {
                after_commit(value);
            }
            return result;
        }
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(0);
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        let response = Arc::new(Mutex::new(Some(response_tx)));
        let task_response = response.clone();
        let rejected_response = response;
        let (job, completion) = CommitterJob::new(
            move || {
                let sent = task_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                    .is_some_and(|response| response.send(task()).is_ok());
                if sent {
                    let _ = ack_rx.recv();
                }
            },
            move |error| {
                if let Some(response) = rejected_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                {
                    let _ = response.send(Err(error));
                }
            },
        );
        self.send_blocking(wrap(job))?;
        let result = response_rx.recv().map_err(|_| {
            Error::Internal("tenant committer actor dropped a blocking response".to_string())
        });
        struct AckOnDrop(Option<std::sync::mpsc::SyncSender<()>>);
        impl Drop for AckOnDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let ack = AckOnDrop(Some(ack_tx));
        if let Ok(Ok(value)) = &result {
            after_commit(value);
        }
        drop(ack);
        let completion = blocking_receive(
            completion,
            "tenant committer actor dropped a blocking completion",
        );
        completion?;
        result?
    }

    pub(crate) async fn submit_async<T, F>(
        &self,
        wrap: impl FnOnce(CommitterJob) -> CommitterMessage,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        if on_actor_task(&self.tenant_id) {
            return task();
        }
        let (response_tx, response_rx) = oneshot::channel();
        let response = Arc::new(Mutex::new(Some(response_tx)));
        let task_response = response.clone();
        let rejected_response = response;
        let (job, completion) = CommitterJob::new(
            move || {
                if let Some(response) = task_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                {
                    let _ = response.send(task());
                }
            },
            move |error| {
                if let Some(response) = rejected_response
                    .lock()
                    .expect("committer response lock should not be poisoned")
                    .take()
                {
                    let _ = response.send(Err(error));
                }
            },
        );
        self.send_async(wrap(job)).await?;
        let result = response_rx.await.map_err(|_| {
            Error::Internal("tenant committer actor dropped an async response".to_string())
        });
        let completion = completion.await.map_err(|_| {
            Error::Internal("tenant committer actor dropped an async completion".to_string())
        });
        completion?;
        result?
    }

    pub(crate) fn depth(&self) -> usize {
        self.sender
            .max_capacity()
            .saturating_sub(self.sender.capacity())
    }

    pub(crate) fn capacity(&self) -> usize {
        self.inbox_capacity
    }

    pub(crate) fn send_timeout(&self) -> Duration {
        self.send_timeout
    }

    pub(crate) fn send_timeout_count(&self) -> u64 {
        self.send_timeout_count.load(Ordering::Relaxed)
    }

    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub(crate) fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

fn blocking_receive<T>(
    mut receiver: oneshot::Receiver<T>,
    dropped_message: &'static str,
) -> Result<T> {
    loop {
        match receiver.try_recv() {
            Ok(result) => return Ok(result),
            Err(oneshot::error::TryRecvError::Closed) => {
                return Err(Error::Internal(dropped_message.to_string()));
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // Sync engine APIs are intentionally callable from a
                // current-thread Tokio context. `blocking_recv` panics in
                // that context, while this bounded park lets the actor on
                // the Engine-owned runtime make progress without entering
                // or blocking that runtime's worker.
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }
    }
}

pub(crate) async fn run_committer_actor(
    runtime: Weak<TenantRuntime>,
    receiver: mpsc::Receiver<CommitterMessage>,
    engine_shutdown: CancellationToken,
    tenant_shutdown: CancellationToken,
    closes_observer_dispatch: bool,
    eviction_registry: TenantEvictionRegistry,
) {
    let tenant_id = runtime
        .upgrade()
        .expect("committer actor runtime must exist at startup")
        .tenant_id()
        .clone();
    COMMITTER_ACTOR_ACTIVE
        .scope(
            tenant_id,
            run_committer_actor_loop(runtime.clone(), receiver, engine_shutdown, tenant_shutdown),
        )
        .await;
    let eviction = runtime.upgrade().and_then(|runtime| {
        if let Some(error) = runtime.committer_fenced_error() {
            // A fence is definitive: the provider CAS rejected and rolled back
            // the transaction. Eviction only surrenders sequence authority;
            // unlike an ambiguous outcome, this path does not probe or claim
            // crash replay.
            begin_definitive_fence_eviction(&runtime, &error);
            runtime.fail_and_drain_mutation_queues(&error);
            Some(runtime)
        } else if runtime.eviction_started() {
            // Internal ordered jobs do not have a route-specific outer caller
            // that can finish eviction after their operation guard drops. The
            // actor is the common owner that survives every durable route.
            let error = runtime.durable_recovery_eviction_error();
            runtime.fail_and_drain_mutation_queues(&error);
            Some(runtime)
        } else {
            None
        }
    });
    if closes_observer_dispatch && let Some(runtime) = runtime.upgrade() {
        runtime.close_committed_mutation_observers();
        if runtime.eviction_started() {
            let _ = runtime
                .wait_for_committed_mutation_observers_drained_for_eviction()
                .await;
        } else {
            runtime
                .wait_for_committed_mutation_observers_drained()
                .await;
        }
    }
    if let Some(runtime) = eviction {
        if !closes_observer_dispatch {
            // The ordered publisher owns observer close/drain. Waiting for its
            // terminal signal also proves it released its TenantRuntime Arc.
            runtime.wait_for_publisher_finished().await;
        }
        runtime.wait_for_operation_drain_for_eviction().await;
        eviction_registry.finish(runtime);
    }
}

async fn run_committer_actor_loop(
    runtime: Weak<TenantRuntime>,
    mut receiver: mpsc::Receiver<CommitterMessage>,
    engine_shutdown: CancellationToken,
    tenant_shutdown: CancellationToken,
) {
    // This task is the tenant's single serial-commit owner. It never retires
    // while the runtime is live, so the old #184 clear/re-check/re-arm wakeup
    // protocol and its lost-wakeup interleaving no longer exist.
    loop {
        let message = tokio::select! {
            message = receiver.recv() => message,
            _ = engine_shutdown.cancelled() => {
                receiver.close();
                receiver.recv().await
            }
            _ = tenant_shutdown.cancelled() => {
                receiver.close();
                receiver.recv().await
            }
        };
        let Some(message) = message else {
            break;
        };
        let Some(runtime) = runtime.upgrade() else {
            break;
        };
        if let Some(error) = runtime.committer_fenced_error() {
            begin_definitive_fence_eviction(&runtime, &error);
            fail_committer_message_during_fence_eviction(runtime.as_ref(), message, &error);
            continue;
        }
        if let CommitterMessage::QueuedBatch {
            engine,
            owns_pending_wake,
        } = message
        {
            struct WorkerRunning<'a>(&'a TenantRuntime);
            impl Drop for WorkerRunning<'_> {
                fn drop(&mut self) {
                    self.0.set_mutation_worker_running(false);
                }
            }
            // Clear only after receiving the message. A producer that already
            // observed the pending wake enqueued before that observation, so
            // this drain sees its work; a later producer installs the next
            // wake. At most one such wake occupies the bounded inbox.
            runtime.accept_queued_committer_batch(owns_pending_wake);
            runtime.record_mutation_worker_start();
            runtime.set_mutation_worker_running(true);
            let _running = WorkerRunning(runtime.as_ref());
            let finish_shutdown_eviction = engine
                .clone()
                .run_one_committer_journal_batch(runtime.clone())
                .await;
            if finish_shutdown_eviction {
                // Fail every residual actor job to wake blocked submitters,
                // then leave through the actor's single eviction finalizer.
                receiver.close();
                while let Some(message) = receiver.recv().await {
                    fail_committer_message_during_shutdown_eviction(runtime.as_ref(), message);
                }
                break;
            }
            continue;
        }
        let job = match message {
            CommitterMessage::DirectCommit(job)
            | CommitterMessage::ExecutionUnitCommit(job)
            | CommitterMessage::JournalProgressSync(job)
            | CommitterMessage::InternalCommit(job) => job,
            CommitterMessage::QueuedBatch { .. } => unreachable!(),
        };
        if runtime.eviction_started() {
            job.fail(runtime.durable_recovery_eviction_error());
            runtime.record_mutation_worker_failure();
            continue;
        }
        if runtime.uses_ordered_publisher() {
            match runtime.send_publisher_ordered_opaque_job(job).await {
                Ok(drained) => {
                    if drained.await.is_err() {
                        runtime.record_mutation_worker_failure();
                    }
                }
                Err((job, error)) => {
                    job.fail(error.clone());
                    runtime.record_mutation_worker_failure();
                    tracing::warn!(
                        tenant = %runtime.tenant_id(),
                        error = %error,
                        "committer opaque job rejected by the ordered publisher"
                    );
                }
            }
            continue;
        }
        let tenant_id = runtime.tenant_id().clone();
        let (task, completed) = job.into_parts();
        let failed = tokio::task::spawn_blocking(move || run_job(&tenant_id, task))
            .await
            .is_err();
        let _ = completed.send(());
        if failed {
            runtime.record_mutation_worker_failure();
        }
    }
}

fn fail_committer_message_during_fence_eviction(
    runtime: &TenantRuntime,
    message: CommitterMessage,
    error: &Error,
) {
    match message {
        CommitterMessage::QueuedBatch {
            owns_pending_wake, ..
        } => {
            runtime.accept_queued_committer_batch(owns_pending_wake);
            runtime.fail_and_drain_mutation_queues(error);
        }
        CommitterMessage::DirectCommit(job)
        | CommitterMessage::ExecutionUnitCommit(job)
        | CommitterMessage::JournalProgressSync(job)
        | CommitterMessage::InternalCommit(job) => {
            job.fail(error.clone());
            runtime.record_mutation_worker_failure();
        }
    }
}

fn fail_committer_message_during_shutdown_eviction(
    runtime: &TenantRuntime,
    message: CommitterMessage,
) {
    match message {
        CommitterMessage::QueuedBatch {
            owns_pending_wake, ..
        } => {
            runtime.accept_queued_committer_batch(owns_pending_wake);
            let error = runtime.durable_recovery_eviction_error();
            runtime.fail_and_drain_mutation_queues(&error);
        }
        CommitterMessage::DirectCommit(job)
        | CommitterMessage::ExecutionUnitCommit(job)
        | CommitterMessage::JournalProgressSync(job)
        | CommitterMessage::InternalCommit(job) => {
            job.fail(runtime.durable_recovery_eviction_error());
            runtime.record_mutation_worker_failure();
        }
    }
}

pub(crate) fn run_job(tenant_id: &TenantId, job: Box<dyn FnOnce() + Send + 'static>) {
    COMMITTER_HANDLER_ACTIVE.with(|active| {
        let previous = active.replace(Some(tenant_id.clone()));
        debug_assert!(previous.is_none(), "committer handler must not nest");
        struct Reset<'a>(&'a std::cell::RefCell<Option<TenantId>>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.replace(None);
            }
        }
        let _reset = Reset(active);
        job();
    });
}

fn assert_not_reentrant(tenant_id: &TenantId) {
    // Audited re-entry points: scheduler jobs, trigger execution, and nested
    // runMutation dispatch all execute on independent tasks; ordered commit
    // fanout only enqueues subscription/trigger work; the installed system
    // projection observer also spawns its async write. Recovery reached from
    // this actor calls `sync_mutation_journal_progress_in_actor` directly.
    // Keep both guards so any future inline path fails loudly in debug builds
    // instead of becoming a send-and-wait self-deadlock.
    debug_assert!(
        COMMITTER_ACTOR_ACTIVE
            .try_with(|active| active != tenant_id)
            .unwrap_or(true)
            && COMMITTER_HANDLER_ACTIVE.with(|active| {
                active
                    .borrow()
                    .as_ref()
                    .is_none_or(|active| active != tenant_id)
            }),
        "committer work must never send-and-wait on its own inbox"
    );
}

fn on_actor_task(tenant_id: &TenantId) -> bool {
    let actor_tenant = COMMITTER_ACTOR_ACTIVE.try_with(Clone::clone).ok();
    let handler_tenant = COMMITTER_HANDLER_ACTIVE.with(|active| active.borrow().clone());
    let inline =
        actor_tenant.as_ref() == Some(tenant_id) || handler_tenant.as_ref() == Some(tenant_id);
    if handler_tenant
        .as_ref()
        .is_some_and(|active| active != tenant_id)
    {
        debug_assert!(
            !inline,
            "cross-tenant committer re-entry must enqueue on the target tenant"
        );
    }
    if inline {
        // Observer callbacks for queued commits intentionally execute after
        // publication on the actor task. Preserve their historical ability
        // to perform a synchronous nested write by handling it directly in
        // the serial loop instead of sending-and-waiting on this same inbox.
        true
    } else {
        false
    }
}

#[cfg(test)]
mod serial_invariant_tests {
    use super::*;

    #[test]
    fn append_validation_rejects_an_interior_sequence_hole() {
        let error =
            validate_append_sequences(SequenceNumber(3), [SequenceNumber(4), SequenceNumber(6)])
                .expect_err("an out-of-order append must fail before persistence");
        assert!(matches!(error, Error::Internal(message) if message.contains("expected 5")));
    }

    #[test]
    fn cross_tenant_handler_reentry_never_inlines_the_other_tenant() {
        let tenant_a = TenantId::new("handler-a").expect("tenant A should build");
        let tenant_b = TenantId::new("handler-b").expect("tenant B should build");
        let tenant_a_for_job = tenant_a.clone();
        let tenant_b_for_job = tenant_b.clone();
        run_job(
            &tenant_a,
            Box::new(move || {
                assert!(on_actor_task(&tenant_a_for_job));
                assert!(
                    !on_actor_task(&tenant_b_for_job),
                    "a handler for tenant A must enqueue on tenant B instead of bypassing B's actor"
                );
            }),
        );
    }
}
