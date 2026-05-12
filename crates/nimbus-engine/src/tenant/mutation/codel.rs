use std::time::{Duration, Instant};

use super::stats::MutationAdmissionPhase;

pub(super) struct CoDelState {
    target: Duration,
    interval: Duration,
    phase: CoDelPhase,
    first_above_time: Option<Instant>,
}

pub(super) enum CoDelPhase {
    Idle,
    Dropping { drop_next: Instant, drop_count: u32 },
}

impl CoDelState {
    pub(super) fn new(target: Duration, interval: Duration) -> Self {
        Self {
            target,
            interval,
            phase: CoDelPhase::Idle,
            first_above_time: None,
        }
    }

    pub(super) fn should_drop(&mut self, now: Instant, enqueued_at: Instant) -> bool {
        let sojourn = now.saturating_duration_since(enqueued_at);
        if sojourn < self.target {
            self.reset();
            return false;
        }

        match &mut self.phase {
            CoDelPhase::Idle => match self.first_above_time {
                None => {
                    self.first_above_time = Some(now + self.interval);
                    false
                }
                Some(first_above_time) if now < first_above_time => false,
                Some(_) => {
                    self.phase = CoDelPhase::Dropping {
                        drop_next: now + codel_drop_interval(self.interval, 1),
                        drop_count: 1,
                    };
                    true
                }
            },
            CoDelPhase::Dropping {
                drop_next,
                drop_count,
            } => {
                if sojourn < self.target {
                    self.reset();
                    return false;
                }
                if now < *drop_next {
                    return false;
                }
                *drop_count = drop_count.saturating_add(1);
                *drop_next = now + codel_drop_interval(self.interval, *drop_count);
                true
            }
        }
    }

    pub(super) fn reset(&mut self) {
        self.phase = CoDelPhase::Idle;
        self.first_above_time = None;
    }

    pub(super) fn phase_stats(&self) -> MutationAdmissionPhase {
        match self.phase {
            CoDelPhase::Idle => MutationAdmissionPhase::Idle,
            CoDelPhase::Dropping { .. } => MutationAdmissionPhase::Dropping,
        }
    }
}

fn codel_drop_interval(interval: Duration, drop_count: u32) -> Duration {
    let divisor = f64::from(drop_count.max(1)).sqrt();
    Duration::from_secs_f64((interval.as_secs_f64() / divisor).max(0.000_001))
}
