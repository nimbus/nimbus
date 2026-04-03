use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;

const EXTERNAL_CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);

type CancelCallback = Box<dyn FnOnce() + Send + 'static>;

enum WatchdogCommand {
    RegisterTimeout {
        id: u64,
        deadline: Instant,
        cancel: CancelCallback,
    },
    RegisterCancellation {
        id: u64,
        cancellation: HostCallCancellation,
        cancel: CancelCallback,
    },
    Cancel {
        id: u64,
        ack: Option<oneshot::Sender<()>>,
    },
    Shutdown,
}

struct CancellationRegistration {
    cancellation: HostCallCancellation,
    cancel: CancelCallback,
}

#[derive(Clone)]
pub(crate) struct WatchdogTimer {
    inner: Arc<WatchdogTimerInner>,
}

struct WatchdogTimerInner {
    next_id: AtomicU64,
    sender: Mutex<Option<mpsc::Sender<WatchdogCommand>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub(crate) struct WatchdogRegistration {
    timer: WatchdogTimer,
    id: Option<u64>,
}

impl WatchdogTimer {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("neovex-runtime-watchdog".to_string())
            .spawn(move || {
                Self::run(receiver);
            })
            .expect("runtime watchdog thread should start");

        Self {
            inner: Arc::new(WatchdogTimerInner {
                next_id: AtomicU64::new(1),
                sender: Mutex::new(Some(sender)),
                thread: Mutex::new(Some(thread)),
            }),
        }
    }

    pub(crate) fn register_timeout<F>(
        &self,
        deadline: Instant,
        cancel: F,
    ) -> Result<WatchdogRegistration>
    where
        F: FnOnce() + Send + 'static,
    {
        let id = self.allocate_id();
        self.send_command(WatchdogCommand::RegisterTimeout {
            id,
            deadline,
            cancel: Box::new(cancel),
        })?;
        Ok(WatchdogRegistration {
            timer: self.clone(),
            id: Some(id),
        })
    }

    pub(crate) fn register_cancellation<F>(
        &self,
        cancellation: HostCallCancellation,
        cancel: F,
    ) -> Result<WatchdogRegistration>
    where
        F: FnOnce() + Send + 'static,
    {
        let id = self.allocate_id();
        self.send_command(WatchdogCommand::RegisterCancellation {
            id,
            cancellation,
            cancel: Box::new(cancel),
        })?;
        Ok(WatchdogRegistration {
            timer: self.clone(),
            id: Some(id),
        })
    }

    pub(crate) fn shutdown(&self) {
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime watchdog sender lock should not be poisoned")
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(WatchdogCommand::Shutdown);
        }
        if let Some(thread) = self
            .inner
            .thread
            .lock()
            .expect("runtime watchdog thread lock should not be poisoned")
            .take()
        {
            let _ = thread.join();
        }
    }

    fn allocate_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn send_command(&self, command: WatchdogCommand) -> Result<()> {
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime watchdog sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime watchdog unexpectedly closed".to_string())
            })?;
        sender.send(command).map_err(|_| {
            NeovexRuntimeError::Contract("runtime watchdog unexpectedly closed".to_string())
        })
    }

    async fn cancel_and_wait(&self, id: u64) {
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime watchdog sender lock should not be poisoned")
            .as_ref()
            .cloned();
        let Some(sender) = sender else {
            return;
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        if sender
            .send(WatchdogCommand::Cancel {
                id,
                ack: Some(ack_tx),
            })
            .is_ok()
        {
            let _ = ack_rx.await;
        }
    }

    fn cancel_without_wait(&self, id: u64) {
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime watchdog sender lock should not be poisoned")
            .as_ref()
            .cloned();
        if let Some(sender) = sender {
            let _ = sender.send(WatchdogCommand::Cancel { id, ack: None });
        }
    }

    fn run(receiver: mpsc::Receiver<WatchdogCommand>) {
        let mut deadlines = BinaryHeap::<Reverse<(Instant, u64)>>::new();
        let mut timeout_handlers = HashMap::<u64, CancelCallback>::new();
        let mut cancellation_handlers = HashMap::<u64, CancellationRegistration>::new();

        loop {
            Self::fire_expired_timeouts(&mut deadlines, &mut timeout_handlers);
            Self::fire_cancelled_registrations(&mut cancellation_handlers);

            let wait = Self::next_wait(
                deadlines.peek().map(|Reverse((deadline, _))| *deadline),
                !cancellation_handlers.is_empty(),
            );

            let command = match wait {
                Some(wait) => match receiver.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match receiver.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                },
            };

            match command {
                Some(WatchdogCommand::RegisterTimeout {
                    id,
                    deadline,
                    cancel,
                }) => {
                    deadlines.push(Reverse((deadline, id)));
                    timeout_handlers.insert(id, cancel);
                }
                Some(WatchdogCommand::RegisterCancellation {
                    id,
                    cancellation,
                    cancel,
                }) => {
                    cancellation_handlers.insert(
                        id,
                        CancellationRegistration {
                            cancellation,
                            cancel,
                        },
                    );
                }
                Some(WatchdogCommand::Cancel { id, ack }) => {
                    timeout_handlers.remove(&id);
                    cancellation_handlers.remove(&id);
                    if let Some(ack) = ack {
                        let _ = ack.send(());
                    }
                }
                Some(WatchdogCommand::Shutdown) => break,
                None => {}
            }
        }
    }

    fn next_wait(next_deadline: Option<Instant>, poll_cancellations: bool) -> Option<Duration> {
        let timeout_wait =
            next_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        match (timeout_wait, poll_cancellations) {
            (Some(timeout_wait), true) => {
                Some(timeout_wait.min(EXTERNAL_CANCELLATION_POLL_INTERVAL))
            }
            (Some(timeout_wait), false) => Some(timeout_wait),
            (None, true) => Some(EXTERNAL_CANCELLATION_POLL_INTERVAL),
            (None, false) => None,
        }
    }

    fn fire_expired_timeouts(
        deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
        timeout_handlers: &mut HashMap<u64, CancelCallback>,
    ) {
        let now = Instant::now();
        while let Some(Reverse((deadline, id))) = deadlines.peek().copied() {
            if deadline > now {
                break;
            }
            deadlines.pop();
            if let Some(cancel) = timeout_handlers.remove(&id) {
                cancel();
            }
        }
    }

    fn fire_cancelled_registrations(
        cancellation_handlers: &mut HashMap<u64, CancellationRegistration>,
    ) {
        let mut fired_ids = Vec::new();
        for (&id, registration) in cancellation_handlers.iter() {
            if registration.cancellation.is_cancelled() {
                fired_ids.push(id);
            }
        }
        for id in fired_ids {
            if let Some(registration) = cancellation_handlers.remove(&id) {
                (registration.cancel)();
            }
        }
    }
}

impl WatchdogRegistration {
    pub(crate) async fn disarm(mut self) {
        if let Some(id) = self.id.take() {
            self.timer.cancel_and_wait(id).await;
        }
    }
}

impl Drop for WatchdogRegistration {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.timer.cancel_without_wait(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::RecvTimeoutError;

    use super::*;

    #[tokio::test]
    async fn timeout_registration_fires() {
        let timer = WatchdogTimer::new();
        let (tx, rx) = mpsc::channel();
        let _registration = timer
            .register_timeout(Instant::now() + Duration::from_millis(20), move || {
                let _ = tx.send(());
            })
            .expect("timeout registration should succeed");

        rx.recv_timeout(Duration::from_millis(250))
            .expect("timeout callback should fire");
        timer.shutdown();
    }

    #[tokio::test]
    async fn disarm_prevents_timeout_callback() {
        let timer = WatchdogTimer::new();
        let (tx, rx) = mpsc::channel();
        let registration = timer
            .register_timeout(Instant::now() + Duration::from_millis(50), move || {
                let _ = tx.send(());
            })
            .expect("timeout registration should succeed");

        registration.disarm().await;
        match rx.recv_timeout(Duration::from_millis(150)) {
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            other => panic!("timeout callback should stay disarmed, got {other:?}"),
        }
        timer.shutdown();
    }

    #[tokio::test]
    async fn external_cancellation_registration_fires() {
        let timer = WatchdogTimer::new();
        let cancellation = HostCallCancellation::default();
        let cancellation_clone = cancellation.clone();
        let (tx, rx) = mpsc::channel();
        let _registration = timer
            .register_cancellation(cancellation, move || {
                let _ = tx.send(());
            })
            .expect("cancellation registration should succeed");

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            cancellation_clone.cancel();
        });

        rx.recv_timeout(Duration::from_millis(250))
            .expect("cancellation callback should fire");
        timer.shutdown();
    }
}
