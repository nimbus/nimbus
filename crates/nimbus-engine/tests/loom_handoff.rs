#![cfg(loom)]

use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Condvar, Mutex};
use loom::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    Accepted,
    TimedOut,
    Closed,
}

#[derive(Debug)]
struct InboxState {
    slot: Option<u8>,
    closed: bool,
    accepted: usize,
    processed: usize,
}

#[derive(Debug)]
struct BoundedInbox {
    state: Mutex<InboxState>,
    changed: Condvar,
}

impl BoundedInbox {
    fn prefilled(message: u8) -> Self {
        Self {
            state: Mutex::new(InboxState {
                slot: Some(message),
                closed: false,
                accepted: 1,
                processed: 0,
            }),
            changed: Condvar::new(),
        }
    }

    fn send_or_timeout(&self, message: u8) -> SendOutcome {
        let mut state = self.state.lock().expect("model inbox lock");
        if state.closed {
            return SendOutcome::Closed;
        }
        if state.slot.is_some() {
            return SendOutcome::TimedOut;
        }
        state.slot = Some(message);
        state.accepted += 1;
        self.changed.notify_all();
        SendOutcome::Accepted
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("model inbox lock");
        state.closed = true;
        self.changed.notify_all();
    }

    fn drain_until_closed(&self) {
        let mut state = self.state.lock().expect("model inbox lock");
        loop {
            if state.slot.take().is_some() {
                state.processed += 1;
                self.changed.notify_all();
                continue;
            }
            if state.closed {
                return;
            }
            state = self.changed.wait(state).expect("model inbox wait");
        }
    }
}

#[test]
fn bounded_actor_handoff_drains_or_rejects_across_shutdown() {
    loom::model(|| {
        let inbox = Arc::new(BoundedInbox::prefilled(1));
        let actor = {
            let inbox = inbox.clone();
            thread::spawn(move || inbox.drain_until_closed())
        };
        let sender = {
            let inbox = inbox.clone();
            thread::spawn(move || inbox.send_or_timeout(2))
        };
        let shutdown = {
            let inbox = inbox.clone();
            thread::spawn(move || inbox.close())
        };

        let outcome = sender.join().expect("sender model thread");
        shutdown.join().expect("shutdown model thread");
        actor.join().expect("actor model thread");

        assert!(matches!(
            outcome,
            SendOutcome::Accepted | SendOutcome::TimedOut | SendOutcome::Closed
        ));
        let state = inbox.state.lock().expect("final model inbox lock");
        assert!(state.closed);
        assert!(state.slot.is_none());
        assert_eq!(state.processed, state.accepted, "accepted work must drain");
        drop(state);
        assert_eq!(inbox.send_or_timeout(3), SendOutcome::Closed);
    });
}

#[test]
fn old_184_check_before_clear_interleaving_strands_work() {
    loom::model(|| {
        let queue_has_work = Arc::new(AtomicBool::new(false));
        let worker_running = Arc::new(AtomicBool::new(true));
        let stale_check_done = Arc::new(AtomicBool::new(false));
        let producer_checked_worker = Arc::new(AtomicBool::new(false));

        let worker = {
            let queue_has_work = queue_has_work.clone();
            let worker_running = worker_running.clone();
            let stale_check_done = stale_check_done.clone();
            let producer_checked_worker = producer_checked_worker.clone();
            thread::spawn(move || {
                assert!(!queue_has_work.load(Ordering::Acquire));
                stale_check_done.store(true, Ordering::Release);
                while !producer_checked_worker.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                worker_running.store(false, Ordering::Release);
            })
        };
        let producer = {
            let queue_has_work = queue_has_work.clone();
            let worker_running = worker_running.clone();
            let stale_check_done = stale_check_done.clone();
            let producer_checked_worker = producer_checked_worker.clone();
            thread::spawn(move || {
                while !stale_check_done.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                queue_has_work.store(true, Ordering::Release);
                assert!(worker_running.load(Ordering::Acquire));
                producer_checked_worker.store(true, Ordering::Release);
            })
        };

        worker.join().expect("old worker model thread");
        producer.join().expect("old producer model thread");
        assert!(queue_has_work.load(Ordering::Acquire));
        assert!(
            !worker_running.load(Ordering::Acquire),
            "the old check-before-clear ordering strands an admitted message"
        );
    });
}

#[test]
fn legacy_clear_then_check_closes_the_184_window() {
    loom::model(|| {
        let queue_has_work = Arc::new(AtomicBool::new(false));
        let worker_running = Arc::new(AtomicBool::new(true));
        let cleared = Arc::new(AtomicBool::new(false));
        let producer_finished = Arc::new(AtomicBool::new(false));

        let worker = {
            let queue_has_work = queue_has_work.clone();
            let worker_running = worker_running.clone();
            let cleared = cleared.clone();
            let producer_finished = producer_finished.clone();
            thread::spawn(move || {
                worker_running.store(false, Ordering::Release);
                cleared.store(true, Ordering::Release);
                while !producer_finished.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                if queue_has_work.load(Ordering::Acquire) {
                    let _ = worker_running.compare_exchange(
                        false,
                        true,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            })
        };
        let producer = {
            let queue_has_work = queue_has_work.clone();
            let worker_running = worker_running.clone();
            let cleared = cleared.clone();
            let producer_finished = producer_finished.clone();
            thread::spawn(move || {
                while !cleared.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                queue_has_work.store(true, Ordering::Release);
                let _ = worker_running.compare_exchange(
                    false,
                    true,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                producer_finished.store(true, Ordering::Release);
            })
        };

        worker.join().expect("fixed worker model thread");
        producer.join().expect("fixed producer model thread");
        assert!(queue_has_work.load(Ordering::Acquire));
        assert!(
            worker_running.load(Ordering::Acquire),
            "clear-before-check guarantees either side owns the wake"
        );
    });
}
