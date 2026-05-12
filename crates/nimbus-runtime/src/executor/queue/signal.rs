use std::sync::{Condvar, Mutex};

pub(crate) struct WorkerActivitySignal {
    generation: Mutex<u64>,
    condvar: Condvar,
    async_notify: tokio::sync::Notify,
}

impl WorkerActivitySignal {
    pub(crate) fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            condvar: Condvar::new(),
            async_notify: tokio::sync::Notify::new(),
        }
    }

    pub(crate) fn current_generation(&self) -> u64 {
        *self
            .generation
            .lock()
            .expect("worker activity generation lock should not be poisoned")
    }

    pub(crate) fn notify(&self) {
        let mut generation = self
            .generation
            .lock()
            .expect("worker activity generation lock should not be poisoned");
        *generation = generation.saturating_add(1);
        self.condvar.notify_all();
        self.async_notify.notify_waiters();
    }

    pub(crate) async fn wait_for_change_async(&self, last_seen_generation: &mut u64) {
        loop {
            let current_generation = self.current_generation();
            if current_generation != *last_seen_generation {
                *last_seen_generation = current_generation;
                return;
            }

            let notified = self.async_notify.notified();
            let current_generation = self.current_generation();
            if current_generation != *last_seen_generation {
                *last_seen_generation = current_generation;
                return;
            }
            notified.await;
        }
    }
}
