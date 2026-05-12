use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::CooperativeWorkerLoop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CooperativeSlotState {
    Runnable,
    InFlight,
    Parked,
}

#[derive(Debug)]
struct CooperativeSlot<T> {
    state: CooperativeSlotState,
    payload: Option<T>,
}

#[derive(Debug)]
pub(super) struct CooperativeRunnableSlot<T> {
    pub(super) slot_id: usize,
    pub(super) payload: T,
}

#[derive(Debug)]
pub(super) struct CooperativeScheduler<T> {
    next_slot_id: usize,
    slots: BTreeMap<usize, CooperativeSlot<T>>,
    run_queue: VecDeque<usize>,
    parked: BTreeSet<usize>,
}

impl CooperativeWorkerLoop {
    pub(super) fn drain_ready_parked_slots(&mut self) {
        self.scheduler
            .resume_parked_where(|invocation| invocation.slot.is_ready_to_resume());
    }
}

impl<T> CooperativeScheduler<T> {
    pub(super) fn new() -> Self {
        Self {
            next_slot_id: 1,
            slots: BTreeMap::new(),
            run_queue: VecDeque::new(),
            parked: BTreeSet::new(),
        }
    }

    pub(super) fn admit_runnable(&mut self, payload: T) -> usize {
        let slot_id = self.next_slot_id;
        self.next_slot_id += 1;
        self.slots.insert(
            slot_id,
            CooperativeSlot {
                state: CooperativeSlotState::Runnable,
                payload: Some(payload),
            },
        );
        self.run_queue.push_back(slot_id);
        slot_id
    }

    pub(super) fn pop_runnable(&mut self) -> Option<CooperativeRunnableSlot<T>> {
        let slot_id = self.run_queue.pop_front()?;
        let slot = self
            .slots
            .get_mut(&slot_id)
            .expect("runnable slot should exist while scheduled");
        debug_assert_eq!(slot.state, CooperativeSlotState::Runnable);
        slot.state = CooperativeSlotState::InFlight;
        let payload = slot
            .payload
            .take()
            .expect("runnable slot should carry its payload");
        Some(CooperativeRunnableSlot { slot_id, payload })
    }

    pub(super) fn finish(&mut self, slot_id: usize) {
        self.parked.remove(&slot_id);
        self.slots.remove(&slot_id);
    }

    pub(super) fn is_idle(&self) -> bool {
        self.run_queue.is_empty() && self.parked.is_empty() && self.slots.is_empty()
    }

    pub(super) fn has_parked(&self) -> bool {
        !self.parked.is_empty()
    }

    pub(super) fn requeue_runnable(&mut self, slot: CooperativeRunnableSlot<T>) {
        let entry = self
            .slots
            .get_mut(&slot.slot_id)
            .expect("slot should exist while requeued");
        entry.state = CooperativeSlotState::Runnable;
        entry.payload = Some(slot.payload);
        self.run_queue.push_back(slot.slot_id);
    }

    pub(super) fn park(&mut self, slot: CooperativeRunnableSlot<T>) {
        let entry = self
            .slots
            .get_mut(&slot.slot_id)
            .expect("slot should exist while parked");
        entry.state = CooperativeSlotState::Parked;
        entry.payload = Some(slot.payload);
        self.parked.insert(slot.slot_id);
    }

    pub(super) fn resume_parked(&mut self, slot_id: usize) {
        if self.parked.remove(&slot_id) {
            let entry = self
                .slots
                .get_mut(&slot_id)
                .expect("parked slot should exist while resumed");
            entry.state = CooperativeSlotState::Runnable;
            self.run_queue.push_back(slot_id);
        }
    }

    pub(super) fn resume_parked_where(&mut self, mut is_ready: impl FnMut(&T) -> bool) {
        let ready: Vec<usize> = self
            .parked
            .iter()
            .copied()
            .filter(|slot_id| {
                self.slots
                    .get(slot_id)
                    .and_then(|slot| slot.payload.as_ref())
                    .is_some_and(&mut is_ready)
            })
            .collect();
        for slot_id in ready {
            self.resume_parked(slot_id);
        }
    }

    #[cfg(test)]
    fn slot_state(&self, slot_id: usize) -> Option<CooperativeSlotState> {
        self.slots.get(&slot_id).map(|slot| slot.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_requeues_runnable_slots_fifo() {
        let mut scheduler = CooperativeScheduler::new();
        let first = scheduler.admit_runnable("first");
        let _second = scheduler.admit_runnable("second");
        let _third = scheduler.admit_runnable("third");

        let head = scheduler.pop_runnable().expect("first runnable slot");
        assert_eq!(head.slot_id, first);
        assert_eq!(head.payload, "first");
        scheduler.requeue_runnable(head);

        let second = scheduler.pop_runnable().expect("second runnable slot");
        assert_eq!(second.payload, "second");
        let third = scheduler.pop_runnable().expect("third runnable slot");
        assert_eq!(third.payload, "third");
        let first_again = scheduler.pop_runnable().expect("requeued first slot");
        assert_eq!(first_again.payload, "first");
    }

    #[test]
    fn scheduler_moves_parked_slots_back_to_fifo_tail() {
        let mut scheduler = CooperativeScheduler::new();
        let first = scheduler.admit_runnable("first");
        let _second = scheduler.admit_runnable("second");

        let head = scheduler.pop_runnable().expect("first runnable slot");
        scheduler.park(head);
        assert_eq!(
            scheduler.slot_state(first),
            Some(CooperativeSlotState::Parked)
        );

        let second = scheduler.pop_runnable().expect("second runnable slot");
        assert_eq!(second.payload, "second");
        scheduler.finish(second.slot_id);

        scheduler.resume_parked(first);
        assert_eq!(
            scheduler.slot_state(first),
            Some(CooperativeSlotState::Runnable)
        );
        let resumed = scheduler.pop_runnable().expect("resumed slot should rerun");
        assert_eq!(resumed.payload, "first");
        scheduler.finish(resumed.slot_id);
        assert!(scheduler.is_idle());
    }
}
