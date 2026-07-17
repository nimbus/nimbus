#![cfg(loom)]

use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Condvar, Mutex};
use loom::thread;

#[derive(Debug)]
struct PublishState {
    pending_low: bool,
    published: Vec<u64>,
    published_through: u64,
}

impl PublishState {
    fn with_pending_low() -> Self {
        Self {
            pending_low: true,
            published: Vec::new(),
            published_through: 1,
        }
    }

    fn record_zero_write_assignment(&mut self, _sequence: u64) {
        // Assignment/coverage is deliberately distinct from publication. A
        // higher zero-write record may complete storage apply while a lower
        // document image is still pending publication.
    }

    fn publish_pending_through(&mut self, applied_head: u64) {
        if self.pending_low && applied_head >= 2 {
            assert!(
                2 > self.published_through,
                "write-log publish order must follow assignment order"
            );
            self.pending_low = false;
            self.published.push(2);
            self.published_through = 2;
        }
        self.published_through = self.published_through.max(applied_head);
    }
}

#[test]
#[should_panic(expected = "write-log publish order must follow assignment order")]
fn eager_zero_write_publish_reproduces_two_batch_order_violation() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(PublishState::with_pending_low()));
        let higher_finished = Arc::new(AtomicBool::new(false));

        let higher_zero_write_batch = {
            let state = state.clone();
            let higher_finished = higher_finished.clone();
            thread::spawn(move || {
                // Pre-fix shape: the later zero-write batch crossed the
                // publish frontier as soon as its storage operation returned.
                state.lock().expect("publish model lock").published_through = 3;
                higher_finished.store(true, Ordering::Release);
            })
        };
        let lower_document_batch = {
            let state = state.clone();
            let higher_finished = higher_finished.clone();
            thread::spawn(move || {
                while !higher_finished.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                state
                    .lock()
                    .expect("publish model lock")
                    .publish_pending_through(3);
            })
        };

        higher_zero_write_batch
            .join()
            .expect("higher zero-write model thread");
        lower_document_batch
            .join()
            .expect("lower document model thread");
    });
}

#[test]
fn applied_prefix_publish_orders_two_batches_across_out_of_order_completion() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(PublishState::with_pending_low()));

        let higher_zero_write_batch = {
            let state = state.clone();
            thread::spawn(move || {
                let mut state = state.lock().expect("publish model lock");
                state.record_zero_write_assignment(3);
                state.publish_pending_through(3);
            })
        };
        let lower_document_batch = {
            let state = state.clone();
            thread::spawn(move || {
                state
                    .lock()
                    .expect("publish model lock")
                    .publish_pending_through(2);
            })
        };

        higher_zero_write_batch
            .join()
            .expect("higher zero-write model thread");
        lower_document_batch
            .join()
            .expect("lower document model thread");

        let state = state.lock().expect("final publish model lock");
        assert_eq!(state.published, vec![2]);
        assert!(!state.pending_low);
        assert_eq!(state.published_through, 3);
    });
}

#[derive(Debug)]
struct AppliedFrontierState {
    pending_low: bool,
    observed_applied_through: u64,
    published_through: u64,
    applied_head: u64,
}

#[derive(Debug)]
struct AppliedFrontier {
    state: Mutex<AppliedFrontierState>,
    applied: Condvar,
}

impl AppliedFrontier {
    fn with_held_pending() -> Self {
        Self {
            state: Mutex::new(AppliedFrontierState {
                pending_low: true,
                observed_applied_through: 1,
                published_through: 1,
                applied_head: 1,
            }),
            applied: Condvar::new(),
        }
    }

    fn sync_progress_through(&self, sequence: u64) {
        let mut state = self.state.lock().expect("applied-frontier model lock");
        state.observed_applied_through = state.observed_applied_through.max(sequence);
        Self::advance_and_notify(&mut state, &self.applied);
    }

    fn publish_held_pending(&self) {
        let mut state = self.state.lock().expect("applied-frontier model lock");
        state.pending_low = false;
        state.observed_applied_through = state.observed_applied_through.max(2);
        Self::advance_and_notify(&mut state, &self.applied);
    }

    fn advance_and_notify(state: &mut AppliedFrontierState, applied: &Condvar) {
        let frontier = if state.pending_low {
            state.observed_applied_through.min(1)
        } else {
            state.observed_applied_through
        };
        state.published_through = state.published_through.max(frontier);
        if state.published_through > state.applied_head {
            state.applied_head = state.published_through;
            applied.notify_all();
        }
    }

    fn wait_for_low(&self) {
        let mut state = self.state.lock().expect("applied-frontier model lock");
        while state.applied_head < 2 {
            state = self.applied.wait(state).expect("applied-frontier wait");
        }
        assert!(
            !state.pending_low,
            "applied waiter woke before the lower pending entry published"
        );
        assert!(state.applied_head <= state.published_through);
    }
}

#[test]
fn progress_sync_cannot_wake_applied_waiter_across_held_pending_entry() {
    loom::model(|| {
        let frontier = Arc::new(AppliedFrontier::with_held_pending());
        let waiter = {
            let frontier = frontier.clone();
            thread::spawn(move || frontier.wait_for_low())
        };
        let progress_sync = {
            let frontier = frontier.clone();
            thread::spawn(move || frontier.sync_progress_through(3))
        };
        let pending_owner = {
            let frontier = frontier.clone();
            thread::spawn(move || frontier.publish_held_pending())
        };

        progress_sync.join().expect("progress-sync model thread");
        pending_owner.join().expect("pending-owner model thread");
        waiter.join().expect("applied-waiter model thread");

        let state = frontier.state.lock().expect("final applied-frontier lock");
        assert!(!state.pending_low);
        assert_eq!(state.published_through, 3);
        assert_eq!(state.applied_head, 3);
    });
}

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
