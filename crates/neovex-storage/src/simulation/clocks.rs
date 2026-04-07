use std::sync::Mutex;
use std::time::Duration;

use neovex_core::Timestamp;

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

pub struct ManualClock {
    now_ms: Mutex<u64>,
}

impl ManualClock {
    pub fn new(now: Timestamp) -> Self {
        Self {
            now_ms: Mutex::new(now.0),
        }
    }

    pub fn set(&self, now: Timestamp) {
        *self
            .now_ms
            .lock()
            .expect("manual clock lock should not be poisoned") = now.0;
    }

    pub fn advance(&self, duration: Duration) -> Timestamp {
        self.advance_ms(duration.as_millis().try_into().unwrap_or(u64::MAX))
    }

    pub fn advance_ms(&self, millis: u64) -> Timestamp {
        let mut now = self
            .now_ms
            .lock()
            .expect("manual clock lock should not be poisoned");
        *now = now.saturating_add(millis);
        Timestamp(*now)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(
            *self
                .now_ms
                .lock()
                .expect("manual clock lock should not be poisoned"),
        )
    }
}
