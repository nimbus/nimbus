use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

pub(super) type CommitterJob = Box<dyn FnOnce() + Send + 'static>;

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

pub(crate) enum CommitterMessage {
    QueuedBatch { engine: Arc<Engine> },
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
    inbox_capacity: usize,
    inbox_depth: AtomicUsize,
    send_timeout_count: AtomicU64,
}

impl CommitterActor {
    pub(crate) fn new() -> Self {
        let inbox_capacity =
            env_positive_usize("NIMBUS_COMMITTER_INBOX_SIZE", DEFAULT_COMMITTER_INBOX_SIZE);
        let (sender, receiver) = mpsc::channel(inbox_capacity);
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            started: AtomicBool::new(false),
            inbox_capacity,
            inbox_depth: AtomicUsize::new(0),
            send_timeout_count: AtomicU64::new(0),
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
        let timeout = self.send_timeout();
        // Reserve the observable depth before publishing the message. The
        // receiver may run immediately after `send` completes, so incrementing
        // afterward can race its decrement and transiently underflow.
        self.inbox_depth.fetch_add(1, Ordering::Relaxed);
        match tokio::time::timeout(timeout, self.sender.send(message)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.inbox_depth.fetch_sub(1, Ordering::Relaxed);
                Err(Error::Internal(format!(
                    "tenant committer actor stopped before accepting {} work",
                    error.0.kind()
                )))
            }
            Err(_) => {
                self.inbox_depth.fetch_sub(1, Ordering::Relaxed);
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

    pub(crate) fn send_blocking(&self, mut message: CommitterMessage) -> Result<()> {
        assert_not_reentrant();
        let timeout = self.send_timeout();
        let deadline = Instant::now() + timeout;
        self.inbox_depth.fetch_add(1, Ordering::Relaxed);
        loop {
            match self.sender.try_send(message) {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TrySendError::Closed(returned)) => {
                    self.inbox_depth.fetch_sub(1, Ordering::Relaxed);
                    return Err(Error::Internal(format!(
                        "tenant committer actor stopped before accepting {} work",
                        returned.kind()
                    )));
                }
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    message = returned;
                    let now = Instant::now();
                    if now >= deadline {
                        self.inbox_depth.fetch_sub(1, Ordering::Relaxed);
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
        let (response_tx, response_rx) = oneshot::channel();
        self.send_blocking(wrap(Box::new(move || {
            let _ = response_tx.send(task());
        })))?;
        let mut response_rx = response_rx;
        loop {
            match response_rx.try_recv() {
                Ok(result) => return result,
                Err(oneshot::error::TryRecvError::Closed) => {
                    return Err(Error::Internal(
                        "tenant committer actor dropped a blocking response".to_string(),
                    ));
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
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(0);
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        self.send_blocking(wrap(Box::new(move || {
            if response_tx.send(task()).is_ok() {
                let _ = ack_rx.recv();
            }
        })))?;
        let result = response_rx.recv().map_err(|_| {
            Error::Internal("tenant committer actor dropped a blocking response".to_string())
        })?;
        struct AckOnDrop(Option<std::sync::mpsc::SyncSender<()>>);
        impl Drop for AckOnDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let ack = AckOnDrop(Some(ack_tx));
        if let Ok(value) = &result {
            after_commit(value);
        }
        drop(ack);
        result
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
        let (response_tx, response_rx) = oneshot::channel();
        self.send_async(wrap(Box::new(move || {
            let _ = response_tx.send(task());
        })))
        .await?;
        response_rx.await.map_err(|_| {
            Error::Internal("tenant committer actor dropped an async response".to_string())
        })?
    }

    pub(crate) fn depth(&self) -> usize {
        self.inbox_depth.load(Ordering::Relaxed)
    }

    pub(crate) fn capacity(&self) -> usize {
        self.inbox_capacity
    }

    pub(crate) fn send_timeout_count(&self) -> u64 {
        self.send_timeout_count.load(Ordering::Relaxed)
    }

    fn received(&self) {
        self.inbox_depth.fetch_sub(1, Ordering::Relaxed);
    }

    fn send_timeout(&self) -> Duration {
        Duration::from_millis(env_nonnegative_u64(
            "NIMBUS_COMMITTER_SEND_TIMEOUT_MS",
            DEFAULT_COMMITTER_SEND_TIMEOUT_MS,
        ))
    }
}

pub(crate) async fn run_committer_actor(
    runtime: Weak<TenantRuntime>,
    receiver: mpsc::Receiver<CommitterMessage>,
    shutdown: CancellationToken,
) {
    COMMITTER_ACTOR_ACTIVE
        .scope((), run_committer_actor_loop(runtime, receiver, shutdown))
        .await;
}

async fn run_committer_actor_loop(
    runtime: Weak<TenantRuntime>,
    mut receiver: mpsc::Receiver<CommitterMessage>,
    shutdown: CancellationToken,
) {
    // This task is the tenant's single serial-commit owner. It never retires
    // while the runtime is live, so the old #184 clear/re-check/re-arm wakeup
    // protocol and its lost-wakeup interleaving no longer exist.
    loop {
        let message = tokio::select! {
            biased;
            message = receiver.recv() => message,
            _ = shutdown.cancelled() => {
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
        runtime.committer.received();
        if let CommitterMessage::QueuedBatch { engine } = message {
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
        if tokio::task::spawn_blocking(move || run_job(job))
            .await
            .is_err()
        {
            runtime.record_mutation_worker_failure();
        }
    }
}

pub(crate) fn run_job(job: CommitterJob) {
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
    debug_assert!(
        COMMITTER_ACTOR_ACTIVE.try_with(|()| ()).is_err()
            && !COMMITTER_HANDLER_ACTIVE.with(std::cell::Cell::get),
        "committer work must never send-and-wait on its own inbox"
    );
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
