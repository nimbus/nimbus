//! Canonical injectable wall-clock seam.
//!
//! `Clock` is the single source of "now" for production code and tests.
//! Reading `SystemTime` is not I/O for the purposes of the crate's zero-I/O
//! rule (no filesystem or network access), so this lives beside
//! [`crate::types::Timestamp`] rather than behind a feature-gated seam.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::types::Timestamp;

/// Source of the current time. Implementations must be cheap and
/// side-effect free beyond reading (or simulating) the clock.
pub trait Clock: Send + Sync {
    /// Current time as milliseconds since the Unix epoch.
    fn now(&self) -> Timestamp;

    /// Current time as milliseconds since the Unix epoch, as a bare `u64`.
    fn now_millis(&self) -> u64 {
        self.now().0
    }

    /// Current time as whole seconds since the Unix epoch, floored.
    ///
    /// This floors rather than rounds: callers that stamp seconds-precision
    /// fields (JWT `iat`/`exp`, SigV4 date headers) previously derived them
    /// via `Duration::as_secs()`, which truncates. `now_millis() / 1000`
    /// preserves that truncation.
    fn now_secs(&self) -> u64 {
        self.now_millis() / 1000
    }

    /// Current time as a [`SystemTime`], for third-party APIs (rustls,
    /// aws-sigv4) that are typed against it.
    ///
    /// This default derives from [`Clock::now`], so it is only as precise
    /// as milliseconds; that is faithful for every current consumer (none
    /// depend on sub-millisecond precision) but callers with tighter
    /// precision needs should not rely on this default.
    fn now_systemtime(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(self.now_millis())
    }
}

/// [`Clock`] backed by the real system clock.
#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// [`Clock`] backed by CI-controlled logical time, for deterministic tests.
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

/// Convenience free function for call sites that have no struct to hold an
/// `Arc<dyn Clock>` (pure plumbing, no test-observable determinism value).
/// Still routes through the one canonical [`SystemClock`] implementation.
pub fn system_now_millis() -> u64 {
    SystemClock.now_millis()
}

/// Convenience free function, seconds variant of [`system_now_millis`].
pub fn system_now_secs() -> u64 {
    SystemClock.now_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_now_millis_and_secs_floor() {
        let clock = ManualClock::new(Timestamp(1_500));
        assert_eq!(clock.now_millis(), 1_500);
        assert_eq!(clock.now_secs(), 1);
    }

    #[test]
    fn manual_clock_advance_and_set() {
        let clock = ManualClock::new(Timestamp(0));
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now(), Timestamp(2_000));
        clock.set(Timestamp(42));
        assert_eq!(clock.now(), Timestamp(42));
    }

    #[test]
    fn now_systemtime_derives_faithfully_from_millis() {
        let clock = ManualClock::new(Timestamp(1_234));
        let expected = SystemTime::UNIX_EPOCH + Duration::from_millis(1_234);
        assert_eq!(clock.now_systemtime(), expected);
    }

    #[test]
    fn system_clock_now_millis_is_close_to_wall_clock() {
        let before = SystemClock.now_millis();
        let observed = system_now_millis();
        let after = SystemClock.now_millis();
        assert!(before <= observed && observed <= after);
    }
}
