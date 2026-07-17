#![cfg(loom)]

use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Condvar, Mutex};
use loom::thread;

#[derive(Debug, Clone, Copy)]
struct CallerPrepare {
    operation: u8,
    observed_version: u64,
}

#[derive(Debug, Default)]
struct PrepareHandoffState {
    ready: Vec<CallerPrepare>,
    assigned: Vec<u8>,
    current_version: u64,
    stale_prepares: usize,
    inline_reprepares: usize,
}

#[derive(Debug, Default)]
struct PrepareHandoff {
    state: Mutex<PrepareHandoffState>,
    ready: Condvar,
}

impl PrepareHandoff {
    fn caller_prepare(&self, operation: u8) {
        let mut state = self.state.lock().expect("prepare-handoff model lock");
        state.ready.push(CallerPrepare {
            operation,
            // Both caller workers prepared from the same published image.
            // Loom varies which prepared result reaches the actor first.
            observed_version: 0,
        });
        self.ready.notify_all();
    }

    fn assign_two(&self) {
        for _ in 0..2 {
            let mut state = self.state.lock().expect("prepare-handoff model lock");
            while state.ready.is_empty() {
                state = self.ready.wait(state).expect("prepare-handoff wait");
            }
            let prepared = state.ready.remove(0);
            assert!(
                !state.assigned.contains(&prepared.operation),
                "a prepared operation must be assigned at most once"
            );
            if prepared.observed_version != state.current_version {
                state.stale_prepares += 1;
                // The actor owns this re-prepare. It rebases the same logical
                // operation on the latest published image without returning it
                // to a caller-side worker or allocating a second assignment.
                state.inline_reprepares += 1;
            }
            state.current_version += 1;
            state.assigned.push(prepared.operation);
        }
    }
}

#[test]
fn concurrent_prepares_racing_inline_reprepare_assign_once_in_either_order() {
    loom::model(|| {
        let handoff = Arc::new(PrepareHandoff::default());
        let first_preparer = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.caller_prepare(1))
        };
        let second_preparer = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.caller_prepare(2))
        };
        let actor = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.assign_two())
        };

        first_preparer.join().expect("first prepare model thread");
        second_preparer.join().expect("second prepare model thread");
        actor.join().expect("prepare actor model thread");

        let state = handoff.state.lock().expect("final prepare-handoff lock");
        assert_eq!(state.assigned.len(), 2, "no caller prepare may be lost");
        assert!(state.assigned.contains(&1));
        assert!(state.assigned.contains(&2));
        assert_eq!(state.current_version, 2);
        assert_eq!(state.stale_prepares, 1);
        assert_eq!(state.inline_reprepares, 1);
    });
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseState {
    Pending,
    Fulfilled,
    Shutdown,
}

#[derive(Debug)]
struct ModelResponse {
    state: Mutex<ResponseState>,
    receiver_live: AtomicBool,
}

impl ModelResponse {
    fn pending() -> Self {
        Self {
            state: Mutex::new(ResponseState::Pending),
            receiver_live: AtomicBool::new(true),
        }
    }

    fn fulfill(&self, response: ResponseState) -> bool {
        assert_ne!(response, ResponseState::Pending);
        let mut state = self.state.lock().expect("model response lock");
        assert_eq!(*state, ResponseState::Pending, "response resolved twice");
        *state = response;
        // A real oneshot send returns the value when its receiver was dropped;
        // it does not panic. Preserve that contract in the model.
        self.receiver_live.load(Ordering::Acquire)
    }

    fn timeout_receiver(&self) -> bool {
        let state = self.state.lock().expect("model response lock");
        if *state == ResponseState::Fulfilled {
            return false;
        }
        self.receiver_live.store(false, Ordering::Release);
        true
    }
}

#[derive(Debug)]
struct TimeoutInboxState {
    slot: Option<Arc<ModelResponse>>,
    sender_finished: bool,
    accepted: usize,
    processed: usize,
}

#[derive(Debug)]
struct TimeoutInbox {
    state: Mutex<TimeoutInboxState>,
    changed: Condvar,
}

impl TimeoutInbox {
    fn prefilled() -> Self {
        Self {
            state: Mutex::new(TimeoutInboxState {
                slot: Some(Arc::new(ModelResponse::pending())),
                sender_finished: false,
                accepted: 1,
                processed: 0,
            }),
            changed: Condvar::new(),
        }
    }

    fn submit_at_deadline(&self, response: Arc<ModelResponse>) -> bool {
        let accepted = {
            let mut state = self.state.lock().expect("timeout inbox lock");
            if state.slot.is_some() {
                false
            } else {
                state.slot = Some(response.clone());
                state.accepted += 1;
                self.changed.notify_all();
                true
            }
        };

        if accepted {
            // Model the deadline racing the actor's response. Fulfillment that
            // won the race is success; otherwise dropping the receiver is safe.
            let _timed_out = response.timeout_receiver();
        } else {
            response.receiver_live.store(false, Ordering::Release);
        }

        let mut state = self.state.lock().expect("timeout inbox lock");
        state.sender_finished = true;
        self.changed.notify_all();
        accepted
    }

    fn drain_until_sender_finished(&self) {
        let mut state = self.state.lock().expect("timeout inbox lock");
        loop {
            if let Some(response) = state.slot.take() {
                state.processed += 1;
                drop(state);
                let _receiver_was_live = response.fulfill(ResponseState::Fulfilled);
                state = self.state.lock().expect("timeout inbox lock");
                self.changed.notify_all();
                continue;
            }
            if state.sender_finished {
                return;
            }
            state = self.changed.wait(state).expect("timeout inbox wait");
        }
    }
}

#[test]
fn send_timeout_racing_actor_drain_never_leaks_or_panics_on_dropped_receiver() {
    loom::model(|| {
        let inbox = Arc::new(TimeoutInbox::prefilled());
        let response = Arc::new(ModelResponse::pending());
        let actor = {
            let inbox = inbox.clone();
            thread::spawn(move || inbox.drain_until_sender_finished())
        };
        let sender = {
            let inbox = inbox.clone();
            let response = response.clone();
            thread::spawn(move || inbox.submit_at_deadline(response))
        };

        let accepted = sender.join().expect("timeout sender model thread");
        actor.join().expect("timeout actor model thread");

        let state = inbox.state.lock().expect("final timeout inbox lock");
        assert_eq!(state.accepted, state.processed);
        assert!(state.slot.is_none());
        drop(state);
        let response_state = *response.state.lock().expect("final response lock");
        if accepted {
            assert_eq!(response_state, ResponseState::Fulfilled);
        } else {
            assert_eq!(response_state, ResponseState::Pending);
            assert!(!response.receiver_live.load(Ordering::Acquire));
        }
    });
}

#[derive(Debug)]
struct ShutdownSubmission {
    accepted: bool,
    response: Arc<ModelResponse>,
}

#[derive(Debug, Default)]
struct ShutdownRaceState {
    closed: bool,
    queued: Vec<Arc<ModelResponse>>,
    accepted: usize,
    resolved_accepted: usize,
}

#[derive(Debug, Default)]
struct ShutdownRace {
    state: Mutex<ShutdownRaceState>,
}

impl ShutdownRace {
    fn submit(&self) -> ShutdownSubmission {
        let response = Arc::new(ModelResponse::pending());
        let mut state = self.state.lock().expect("shutdown race lock");
        if state.closed {
            let _ = response.fulfill(ResponseState::Shutdown);
            return ShutdownSubmission {
                accepted: false,
                response,
            };
        }
        state.accepted += 1;
        state.queued.push(response.clone());
        ShutdownSubmission {
            accepted: true,
            response,
        }
    }

    fn process_one_then_shutdown(&self) {
        let completed = {
            let mut state = self.state.lock().expect("shutdown race lock");
            state.queued.pop()
        };
        if let Some(response) = completed {
            let _ = response.fulfill(ResponseState::Fulfilled);
            self.state
                .lock()
                .expect("shutdown race lock")
                .resolved_accepted += 1;
        }
        thread::yield_now();
        let pending = {
            let mut state = self.state.lock().expect("shutdown race lock");
            state.closed = true;
            std::mem::take(&mut state.queued)
        };
        for response in pending {
            let _ = response.fulfill(ResponseState::Shutdown);
            self.state
                .lock()
                .expect("shutdown race lock")
                .resolved_accepted += 1;
        }
    }
}

#[test]
fn shutdown_racing_prepared_submissions_resolves_every_accepted_message() {
    loom::model(|| {
        let race = Arc::new(ShutdownRace::default());
        let first_sender = {
            let race = race.clone();
            thread::spawn(move || race.submit())
        };
        let second_sender = {
            let race = race.clone();
            thread::spawn(move || race.submit())
        };
        let actor = {
            let race = race.clone();
            thread::spawn(move || race.process_one_then_shutdown())
        };

        let submissions = [
            first_sender.join().expect("first shutdown sender"),
            second_sender.join().expect("second shutdown sender"),
        ];
        actor.join().expect("shutdown actor model thread");

        let state = race.state.lock().expect("final shutdown race lock");
        assert!(state.closed);
        assert!(state.queued.is_empty());
        assert_eq!(state.accepted, state.resolved_accepted);
        drop(state);
        for submission in submissions {
            let response = *submission
                .response
                .state
                .lock()
                .expect("submission response");
            assert_ne!(response, ResponseState::Pending);
            if submission.accepted {
                assert!(matches!(
                    response,
                    ResponseState::Fulfilled | ResponseState::Shutdown
                ));
            } else {
                assert_eq!(response, ResponseState::Shutdown);
            }
        }
    });
}

#[derive(Debug)]
struct InlineAppliedState {
    stale: bool,
    reprepared: bool,
    observed_applied_through: u64,
    published_through: u64,
    applied_head: u64,
}

#[derive(Debug)]
struct InlineAppliedHandoff {
    state: Mutex<InlineAppliedState>,
    applied: Condvar,
}

impl InlineAppliedHandoff {
    fn new() -> Self {
        Self {
            state: Mutex::new(InlineAppliedState {
                stale: true,
                reprepared: false,
                observed_applied_through: 1,
                published_through: 1,
                applied_head: 1,
            }),
            applied: Condvar::new(),
        }
    }

    fn inline_reprepare_and_publish(&self) {
        {
            let mut state = self.state.lock().expect("inline applied lock");
            assert!(state.stale);
            state.reprepared = true;
            state.stale = false;
        }
        thread::yield_now();
        let mut state = self.state.lock().expect("inline applied lock");
        state.published_through = 2;
        Self::advance_and_notify(&mut state, &self.applied);
    }

    fn observe_storage_apply(&self) {
        let mut state = self.state.lock().expect("inline applied lock");
        state.observed_applied_through = 2;
        Self::advance_and_notify(&mut state, &self.applied);
    }

    fn advance_and_notify(state: &mut InlineAppliedState, applied: &Condvar) {
        let visible = state.observed_applied_through.min(state.published_through);
        if visible > state.applied_head {
            state.applied_head = visible;
            applied.notify_all();
        }
    }

    fn wait_for_two(&self) {
        let mut state = self.state.lock().expect("inline applied lock");
        while state.applied_head < 2 {
            state = self.applied.wait(state).expect("inline applied wait");
        }
        assert!(state.reprepared);
        assert_eq!(state.published_through, 2);
    }
}

#[test]
fn applied_waiter_wakes_after_inline_reprepare_publishes_target_sequence() {
    loom::model(|| {
        let handoff = Arc::new(InlineAppliedHandoff::new());
        let waiter = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.wait_for_two())
        };
        let actor = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.inline_reprepare_and_publish())
        };
        let storage = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.observe_storage_apply())
        };

        actor.join().expect("inline reprepare actor model thread");
        storage.join().expect("storage progress model thread");
        waiter.join().expect("inline applied waiter model thread");
        let state = handoff.state.lock().expect("final inline applied lock");
        assert_eq!(state.applied_head, 2);
        assert!(state.applied_head <= state.published_through);
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

#[derive(Debug, Default)]
struct PublisherQueueState {
    queue: Vec<u8>,
    published: Vec<u8>,
    closed: bool,
}

#[derive(Debug, Default)]
struct PublisherQueueHandoff {
    state: Mutex<PublisherQueueState>,
    ready: Condvar,
}

impl PublisherQueueHandoff {
    fn enqueue(&self, batch: u8) -> bool {
        let mut state = self.state.lock().expect("publisher handoff lock");
        if state.closed {
            return false;
        }
        state.queue.push(batch);
        self.ready.notify_all();
        true
    }

    fn drain_until_shutdown(&self) {
        loop {
            let mut state = self.state.lock().expect("publisher drain lock");
            while state.queue.is_empty() && !state.closed {
                state = self.ready.wait(state).expect("publisher drain wait");
            }
            if let Some(batch) = state.queue.first().copied() {
                state.queue.remove(0);
                if let Some(previous) = state.published.last() {
                    assert!(*previous < batch, "publisher batches must drain in order");
                }
                state.published.push(batch);
            } else if state.closed {
                return;
            }
        }
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().expect("publisher shutdown lock");
        state.closed = true;
        self.ready.notify_all();
    }
}

#[test]
fn actor_to_publisher_enqueue_drain_shutdown_loses_no_accepted_batch() {
    loom::model(|| {
        let handoff = Arc::new(PublisherQueueHandoff::default());
        let first_accepted = Arc::new(AtomicBool::new(false));
        let second_accepted = Arc::new(AtomicBool::new(false));

        let actor = {
            let handoff = handoff.clone();
            let first_accepted = first_accepted.clone();
            let second_accepted = second_accepted.clone();
            thread::spawn(move || {
                first_accepted.store(handoff.enqueue(1), Ordering::Release);
                second_accepted.store(handoff.enqueue(2), Ordering::Release);
            })
        };
        let publisher = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.drain_until_shutdown())
        };
        let shutdown = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.shutdown())
        };

        actor.join().expect("publisher model actor thread");
        shutdown.join().expect("publisher model shutdown thread");
        publisher.join().expect("publisher model drain thread");

        let state = handoff.state.lock().expect("final publisher model lock");
        assert!(state.queue.is_empty());
        assert_eq!(
            state.published.contains(&1),
            first_accepted.load(Ordering::Acquire),
            "the first accepted batch must publish exactly once"
        );
        assert_eq!(
            state.published.contains(&2),
            second_accepted.load(Ordering::Acquire),
            "the second accepted batch must publish exactly once"
        );
        assert!(
            !second_accepted.load(Ordering::Acquire) || first_accepted.load(Ordering::Acquire),
            "one actor cannot accept its second batch after rejecting its first"
        );
    });
}

#[derive(Debug, Default)]
struct ObserverDispatchState {
    queue: Vec<u64>,
    observed: Vec<u64>,
    closed: bool,
    drained: bool,
}

#[derive(Debug, Default)]
struct ObserverDispatchHandoff {
    state: Mutex<ObserverDispatchState>,
    ready: Condvar,
}

impl ObserverDispatchHandoff {
    fn enqueue(&self, sequence: u64) {
        let mut state = self.state.lock().expect("observer handoff lock");
        assert!(
            !state.closed,
            "observer work cannot follow the close marker"
        );
        state.queue.push(sequence);
        self.ready.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock().expect("observer close lock");
        state.closed = true;
        self.ready.notify_all();
    }

    fn dispatch_until_drained(&self) {
        loop {
            let mut state = self.state.lock().expect("observer dispatch lock");
            while state.queue.is_empty() && !state.closed {
                state = self.ready.wait(state).expect("observer dispatch wait");
            }
            if let Some(sequence) = state.queue.first().copied() {
                state.queue.remove(0);
                if let Some(previous) = state.observed.last() {
                    assert!(*previous < sequence, "observers must see commit order");
                }
                state.observed.push(sequence);
                drop(state);
                thread::yield_now();
                continue;
            }
            state.drained = true;
            self.ready.notify_all();
            return;
        }
    }
}

#[test]
fn ordered_observer_dispatch_drains_every_commit_before_shutdown_returns() {
    loom::model(|| {
        let handoff = Arc::new(ObserverDispatchHandoff::default());
        let publisher = {
            let handoff = handoff.clone();
            thread::spawn(move || {
                handoff.enqueue(1);
                handoff.enqueue(2);
                handoff.close();
            })
        };
        let dispatcher = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.dispatch_until_drained())
        };
        publisher.join().expect("observer publisher model thread");
        dispatcher.join().expect("observer dispatcher model thread");

        let state = handoff.state.lock().expect("final observer model lock");
        assert_eq!(state.observed, vec![1, 2]);
        assert!(state.drained);
        assert!(state.closed);
        assert!(state.queue.is_empty());
    });
}

#[derive(Debug, Default)]
struct TenantAwareReentryState {
    tenant_b_queued: bool,
    tenant_b_committing: bool,
    tenant_b_commits: usize,
}

#[derive(Debug, Default)]
struct TenantAwareReentry {
    state: Mutex<TenantAwareReentryState>,
    ready: Condvar,
}

impl TenantAwareReentry {
    fn write_from_handler(&self, active_tenant: u8, target_tenant: u8) {
        let mut state = self.state.lock().expect("tenant-aware reentry lock");
        if active_tenant == target_tenant {
            assert!(!state.tenant_b_committing);
            state.tenant_b_commits += 1;
        } else {
            assert_eq!(target_tenant, 2, "the model targets tenant B");
            state.tenant_b_queued = true;
            self.ready.notify_all();
        }
    }

    fn commit_tenant_b_from_its_actor(&self) {
        let mut state = self.state.lock().expect("tenant B actor lock");
        while !state.tenant_b_queued {
            state = self.ready.wait(state).expect("tenant B actor wait");
        }
        assert!(!state.tenant_b_committing);
        state.tenant_b_committing = true;
        state.tenant_b_queued = false;
        drop(state);
        thread::yield_now();
        let mut state = self.state.lock().expect("tenant B completion lock");
        assert!(state.tenant_b_committing);
        state.tenant_b_commits += 1;
        state.tenant_b_committing = false;
    }
}

#[test]
fn cross_tenant_handler_write_never_bypasses_the_target_tenant_actor() {
    loom::model(|| {
        let handoff = Arc::new(TenantAwareReentry::default());
        let handler = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.write_from_handler(1, 2))
        };
        let tenant_b_actor = {
            let handoff = handoff.clone();
            thread::spawn(move || handoff.commit_tenant_b_from_its_actor())
        };

        handler.join().expect("tenant A handler model thread");
        tenant_b_actor.join().expect("tenant B actor model thread");

        let state = handoff.state.lock().expect("final tenant reentry lock");
        assert!(!state.tenant_b_queued);
        assert!(!state.tenant_b_committing);
        assert_eq!(state.tenant_b_commits, 1);
    });
}

#[derive(Debug, Clone, Copy)]
enum RecoveryQueueMessage {
    Batch(u64),
    Fence,
}

#[derive(Debug)]
struct AssignmentRecoveryState {
    durable_head: u64,
    assigned_through: u64,
    pending: Vec<u64>,
    queue: Vec<RecoveryQueueMessage>,
}

#[test]
fn definitive_recovery_racing_actor_reassignment_keeps_one_contiguous_suffix() {
    loom::model(|| {
        let gate = Arc::new(Mutex::new(AssignmentRecoveryState {
            durable_head: 4,
            assigned_through: 8,
            pending: vec![5, 6, 7, 8],
            queue: vec![
                RecoveryQueueMessage::Batch(5),
                RecoveryQueueMessage::Fence,
                RecoveryQueueMessage::Batch(7),
            ],
        }));

        let recovery = {
            let gate = gate.clone();
            thread::spawn(move || {
                let mut state = gate.lock().expect("assignment recovery gate");
                let drained = std::mem::take(&mut state.queue);
                let first_failed = drained
                    .iter()
                    .filter_map(|message| match message {
                        RecoveryQueueMessage::Batch(first) => Some(*first),
                        RecoveryQueueMessage::Fence => None,
                    })
                    .min()
                    .expect("the failed suffix contains a batch");
                state.pending.retain(|sequence| *sequence < first_failed);
                state.assigned_through =
                    state.pending.last().copied().unwrap_or(state.durable_head);
            })
        };
        let actor = {
            let gate = gate.clone();
            thread::spawn(move || {
                let mut state = gate.lock().expect("actor assignment gate");
                let sequence = state.assigned_through + 1;
                state.pending.push(sequence);
                state.assigned_through = sequence;
                state.queue.push(RecoveryQueueMessage::Batch(sequence));
            })
        };

        recovery.join().expect("recovery model thread");
        actor.join().expect("actor model thread");
        let state = gate.lock().expect("final assignment recovery state");
        assert!(matches!(state.pending.as_slice(), [] | [5]));
        assert_eq!(
            state.assigned_through,
            state.pending.last().copied().unwrap_or(state.durable_head)
        );
        assert!(
            state
                .pending
                .windows(2)
                .all(|window| window[1] == window[0] + 1)
        );
        assert!(state.queue.iter().all(|message| {
            matches!(message, RecoveryQueueMessage::Batch(sequence) if state.pending.contains(sequence))
        }));
    });
}
