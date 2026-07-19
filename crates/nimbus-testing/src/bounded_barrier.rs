use std::sync::{Condvar, Mutex};
use std::time::Duration;

const TEST_BARRIER_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
struct BarrierState {
    arrived: usize,
    generation: u64,
}

/// A reusable test barrier that turns a missing participant into a loud
/// failure instead of parking the remaining test threads forever.
pub struct BoundedTestBarrier {
    participants: usize,
    state: Mutex<BarrierState>,
    changed: Condvar,
}

impl BoundedTestBarrier {
    pub fn new(participants: usize) -> Self {
        assert!(participants > 0, "a test barrier needs a participant");
        Self {
            participants,
            state: Mutex::new(BarrierState::default()),
            changed: Condvar::new(),
        }
    }

    pub fn wait(&self) {
        let mut state = self
            .state
            .lock()
            .expect("bounded test barrier lock should not be poisoned");
        let generation = state.generation;
        state.arrived += 1;
        if state.arrived == self.participants {
            state.arrived = 0;
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
            return;
        }
        let (state, _) = self
            .changed
            .wait_timeout_while(state, TEST_BARRIER_TIMEOUT, |state| {
                state.generation == generation
            })
            .expect("bounded test barrier wait should not be poisoned");
        assert_ne!(
            state.generation, generation,
            "test barrier generation {generation} did not receive all {} participants within {TEST_BARRIER_TIMEOUT:?}",
            self.participants
        );
    }
}
