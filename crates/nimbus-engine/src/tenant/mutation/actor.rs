use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, SequenceNumber};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::Engine;

use super::super::TenantRuntime;

const DEFAULT_COMMITTER_INBOX_SIZE: usize = 128;
const DEFAULT_COMMITTER_SEND_TIMEOUT_MS: u64 = 500;

tokio::task_local! {
    static COMMITTER_ACTOR_ACTIVE: ();
}

thread_local! {
    static COMMITTER_HANDLER_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct CommitterJob {
    task: Box<dyn FnOnce() + Send + 'static>,
    completed: oneshot::Sender<()>,
}

impl CommitterJob {
    fn new(task: impl FnOnce() + Send + 'static) -> (Self, oneshot::Receiver<()>) {
        let (completed, completion) = oneshot::channel();
        (
            Self {
                task: Box::new(task),
                completed,
            },
            completion,
        )
    }

    pub(crate) fn into_parts(self) -> (Box<dyn FnOnce() + Send + 'static>, oneshot::Sender<()>) {
        (self.task, self.completed)
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
    InternalSerial(CommitterJob),
}

impl CommitterMessage {
    fn kind(&self) -> &'static str {
        match self {
            Self::QueuedBatch { .. } => "queued-batch",
            Self::DirectCommit(_) => "direct",
            Self::ExecutionUnitCommit(_) => "execution-unit",
            Self::JournalProgressSync(_) => "journal-progress",
            Self::InternalSerial(_) => "internal",
        }
    }
}

pub(crate) struct CommitterActor {
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
    pub(crate) fn new() -> Self {
        let inbox_capacity =
            env_positive_usize("NIMBUS_COMMITTER_INBOX_SIZE", DEFAULT_COMMITTER_INBOX_SIZE);
        let send_timeout = Duration::from_millis(env_nonnegative_u64(
            "NIMBUS_COMMITTER_SEND_TIMEOUT_MS",
            DEFAULT_COMMITTER_SEND_TIMEOUT_MS,
        ));
        let (sender, receiver) = mpsc::channel(inbox_capacity);
        Self {
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
        assert_not_reentrant();
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
        assert_not_reentrant();
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
        assert_not_reentrant();
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
        if on_actor_task() {
            return task();
        }
        let (response_tx, response_rx) = oneshot::channel();
        let (job, completion) = CommitterJob::new(move || {
            let _ = response_tx.send(task());
        });
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
        if on_actor_task() {
            let result = task();
            if let Ok(value) = &result {
                after_commit(value);
            }
            return result;
        }
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(0);
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        let (job, completion) = CommitterJob::new(move || {
            if response_tx.send(task()).is_ok() {
                let _ = ack_rx.recv();
            }
        });
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
        if on_actor_task() {
            return task();
        }
        let (response_tx, response_rx) = oneshot::channel();
        let (job, completion) = CommitterJob::new(move || {
            let _ = response_tx.send(task());
        });
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
) {
    COMMITTER_ACTOR_ACTIVE
        .scope(
            (),
            run_committer_actor_loop(runtime, receiver, engine_shutdown, tenant_shutdown),
        )
        .await;
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
            engine
                .run_one_committer_journal_batch(runtime.clone())
                .await;
            continue;
        }
        let job = match message {
            CommitterMessage::DirectCommit(job)
            | CommitterMessage::ExecutionUnitCommit(job)
            | CommitterMessage::JournalProgressSync(job)
            | CommitterMessage::InternalSerial(job) => job,
            CommitterMessage::QueuedBatch { .. } => unreachable!(),
        };
        if runtime.store.has_process_local_sequence_authority() {
            match runtime.send_publisher_serial_job(job).await {
                Ok(drained) => {
                    if drained.await.is_err() {
                        runtime.record_mutation_worker_failure();
                    }
                }
                Err((job, error)) => {
                    let (task, completed) = job.into_parts();
                    drop(task);
                    let _ = completed.send(());
                    runtime.record_mutation_worker_failure();
                    tracing::warn!(
                        tenant = %runtime.tenant_id(),
                        error = %error,
                        "committer serial job rejected by the ordered publisher"
                    );
                }
            }
            continue;
        }
        let CommitterJob { task, completed } = job;
        let failed = tokio::task::spawn_blocking(move || run_job(task))
            .await
            .is_err();
        let _ = completed.send(());
        if failed {
            runtime.record_mutation_worker_failure();
        }
    }
}

pub(crate) fn run_job(job: Box<dyn FnOnce() + Send + 'static>) {
    COMMITTER_HANDLER_ACTIVE.with(|active| {
        debug_assert!(!active.replace(true), "committer handler must not nest");
        struct Reset<'a>(&'a std::cell::Cell<bool>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _reset = Reset(active);
        job();
    });
}

fn assert_not_reentrant() {
    // Audited re-entry points: scheduler jobs, trigger execution, and nested
    // runMutation dispatch all execute on independent tasks; ordered commit
    // fanout only enqueues subscription/trigger work; the installed system
    // projection observer also spawns its async write. Recovery reached from
    // this actor calls `sync_mutation_journal_progress_in_actor` directly.
    // Keep both guards so any future inline path fails loudly in debug builds
    // instead of becoming a send-and-wait self-deadlock.
    debug_assert!(
        COMMITTER_ACTOR_ACTIVE.try_with(|()| ()).is_err()
            && !COMMITTER_HANDLER_ACTIVE.with(std::cell::Cell::get),
        "committer work must never send-and-wait on its own inbox"
    );
}

fn on_actor_task() -> bool {
    if COMMITTER_ACTOR_ACTIVE.try_with(|()| ()).is_ok()
        || COMMITTER_HANDLER_ACTIVE.with(std::cell::Cell::get)
    {
        // Observer callbacks for queued commits intentionally execute after
        // publication on the actor task. Preserve their historical ability
        // to perform a synchronous nested write by handling it directly in
        // the serial loop instead of sending-and-waiting on this same inbox.
        true
    } else {
        false
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
}
