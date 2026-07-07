//! Shared armed/entered/released condvar pause barrier.
//!
//! Tenant background workers use this pattern for deterministic test
//! synchronization: a test arms the barrier, the worker thread enters it at
//! a known point and blocks, the test observes the entry and asserts state,
//! then releases the worker. `S` is the payload the worker reports on entry
//! (`()` when the caller only needs to know *that* the worker entered;
//! `SequenceNumber` for the subscription publish barrier, which needs to
//! know *which* delivery sequence is paused).

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct PauseBarrierControl<S> {
    armed: bool,
    entered: Option<S>,
    released: bool,
}

impl<S> Default for PauseBarrierControl<S> {
    fn default() -> Self {
        Self {
            armed: false,
            entered: None,
            released: false,
        }
    }
}

/// Worker-facing side of the barrier: checked from inside the background
/// thread's run loop.
#[derive(Debug)]
pub(crate) struct PauseBarrier<S = ()> {
    control: Mutex<PauseBarrierControl<S>>,
    condvar: Condvar,
}

impl<S> Default for PauseBarrier<S> {
    fn default() -> Self {
        Self {
            control: Mutex::new(PauseBarrierControl::default()),
            condvar: Condvar::new(),
        }
    }
}

impl<S> PauseBarrier<S> {
    /// Blocks the calling (worker) thread if the barrier is armed and has
    /// not yet recorded an entry for the current arm cycle, reporting
    /// `payload` to whoever is waiting via `wait_until_entered`. No-op if
    /// not armed, or if this arm cycle already has a recorded entry.
    pub(crate) fn wait_if_armed(&self, payload: S) {
        let mut control = self
            .control
            .lock()
            .expect("pause barrier lock should not be poisoned");
        if !control.armed || control.entered.is_some() {
            return;
        }
        control.entered = Some(payload);
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("pause barrier wait should not be poisoned");
        }
        *control = PauseBarrierControl::default();
    }

    /// Releases a currently-armed, not-yet-released barrier so a worker
    /// parked in `wait_if_armed` does not hang forever during shutdown.
    pub(crate) fn release_for_shutdown(&self) {
        let mut control = self
            .control
            .lock()
            .expect("pause barrier lock should not be poisoned");
        if control.armed && !control.released {
            control.released = true;
            self.condvar.notify_all();
        }
    }
}

/// Test-facing control handle for a `PauseBarrier`.
#[derive(Debug, Clone)]
pub(crate) struct PauseBarrierHandle<S = ()> {
    state: Arc<PauseBarrier<S>>,
}

impl<S: Clone> PauseBarrierHandle<S> {
    pub(crate) fn new(state: Arc<PauseBarrier<S>>) -> Self {
        Self { state }
    }

    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("pause barrier lock should not be poisoned");
        control.armed = true;
        control.entered = None;
        control.released = false;
    }

    /// Waits until the worker enters the barrier (or `timeout` elapses),
    /// returning the payload the worker reported, if any.
    pub(crate) fn wait_until_entered(&self, timeout: Duration) -> Option<S> {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("pause barrier lock should not be poisoned");
        while control.armed && control.entered.is_none() {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return control.entered.clone();
            };
            let (next_control, wait_result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("pause barrier wait should not be poisoned");
            control = next_control;
            if wait_result.timed_out() {
                return control.entered.clone();
            }
        }
        control.entered.clone()
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("pause barrier lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::*;

    #[test]
    fn unit_payload_barrier_reports_entry_and_unblocks_on_release() {
        let state: Arc<PauseBarrier> = Arc::new(PauseBarrier::default());
        let handle = PauseBarrierHandle::new(state.clone());
        handle.arm();

        let entered_worker = Arc::new(AtomicBool::new(false));
        let entered_worker_clone = entered_worker.clone();
        let worker = std::thread::spawn(move || {
            state.wait_if_armed(());
            entered_worker_clone.store(true, Ordering::SeqCst);
        });

        let entered = handle.wait_until_entered(Duration::from_secs(1));
        assert_eq!(entered, Some(()));
        assert!(
            !entered_worker.load(Ordering::SeqCst),
            "worker should still be blocked until release"
        );

        handle.release();
        worker.join().expect("worker thread should join");
        assert!(entered_worker.load(Ordering::SeqCst));
    }

    #[test]
    fn typed_payload_barrier_reports_the_entered_value() {
        let state: Arc<PauseBarrier<u64>> = Arc::new(PauseBarrier::default());
        let handle = PauseBarrierHandle::new(state.clone());
        handle.arm();

        let worker = std::thread::spawn(move || {
            state.wait_if_armed(42);
        });

        assert_eq!(handle.wait_until_entered(Duration::from_secs(1)), Some(42));
        handle.release();
        worker.join().expect("worker thread should join");
    }

    #[test]
    fn release_for_shutdown_unblocks_an_armed_but_unentered_barrier() {
        let state: Arc<PauseBarrier> = Arc::new(PauseBarrier::default());
        let handle = PauseBarrierHandle::new(state.clone());
        handle.arm();

        let state_for_worker = state.clone();
        let worker = std::thread::spawn(move || {
            state_for_worker.wait_if_armed(());
        });

        handle
            .wait_until_entered(Duration::from_secs(1))
            .expect("worker should enter the barrier");
        state.release_for_shutdown();
        worker
            .join()
            .expect("release_for_shutdown should unblock the worker");
    }

    #[test]
    fn not_armed_barrier_never_blocks() {
        let state: Arc<PauseBarrier> = Arc::new(PauseBarrier::default());
        state.wait_if_armed(());
    }
}
