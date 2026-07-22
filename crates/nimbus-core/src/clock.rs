//! Canonical injectable wall-clock seam.
//!
//! [`WallClock`] represents serializable Unix-epoch observations that may move
//! forward or backward. [`MonotonicClock`] represents opaque process-local
//! observations used only for elapsed-time policy. Neither seam owns waiting.
//!
//! Reading `SystemTime` or `Instant` is not I/O for the purposes of the crate's
//! zero-I/O rule (no filesystem or network access), so both seams live beside
//! [`crate::types::Timestamp`] rather than behind a feature-gated adapter.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::types::Timestamp;

/// Source of the current time. Implementations must be cheap and
/// side-effect free beyond reading (or simulating) the clock.
pub trait WallClock: Send + Sync {
    /// Current time as milliseconds since the Unix epoch.
    fn now(&self) -> Timestamp;

    /// Current time as milliseconds since the Unix epoch, as a bare `u64`.
    fn now_millis(&self) -> u64 {
        self.now().as_unix_millis()
    }

    /// Current time as whole seconds since the Unix epoch, floored.
    ///
    /// This floors rather than rounds: callers that stamp seconds-precision
    /// fields (JWT `iat`/`exp`, SigV4 date headers) previously derived them
    /// via `Duration::as_secs()`, which truncates. `now_millis() / 1000`
    /// preserves that truncation.
    fn now_secs(&self) -> u64 {
        self.now().as_unix_secs_floor()
    }

    /// Current time as a [`SystemTime`], for third-party APIs (rustls,
    /// aws-sigv4) that are typed against it.
    ///
    /// This default derives from [`WallClock::now`], so it is only as precise
    /// as milliseconds; that is faithful for every current consumer (none
    /// depend on sub-millisecond precision) but callers with tighter
    /// precision needs should not rely on this default.
    fn now_systemtime(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(self.now_millis())
    }
}

/// [`WallClock`] backed by the real system clock.
#[derive(Default)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// [`WallClock`] backed by controlled epoch time for deterministic tests.
pub struct ManualWallClock {
    now_ms: Mutex<u64>,
}

impl ManualWallClock {
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

impl WallClock for ManualWallClock {
    fn now(&self) -> Timestamp {
        Timestamp(
            *self
                .now_ms
                .lock()
                .expect("manual clock lock should not be poisoned"),
        )
    }
}

/// Source of opaque process-local monotonic observations.
///
/// Returned instants must never move backward and must not be serialized,
/// logged as epoch values, or compared across processes.
pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Instant;
}

/// [`MonotonicClock`] backed by [`Instant::now`].
#[derive(Default)]
pub struct SystemMonotonicClock;

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Deterministic forward-only [`MonotonicClock`].
pub struct ManualMonotonicClock {
    now: Mutex<Instant>,
}

impl Default for ManualMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ManualMonotonicClock {
    pub fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    pub fn advance(&self, duration: Duration) -> Instant {
        let mut now = self
            .now
            .lock()
            .expect("manual monotonic clock lock should not be poisoned");
        *now = now
            .checked_add(duration)
            .expect("manual monotonic clock must remain representable");
        *now
    }
}

impl MonotonicClock for ManualMonotonicClock {
    fn now(&self) -> Instant {
        *self
            .now
            .lock()
            .expect("manual monotonic clock lock should not be poisoned")
    }
}

/// Convenience free function for call sites that have no struct to hold an
/// `Arc<dyn WallClock>` (pure plumbing, no test-observable determinism value).
/// Still routes through the canonical [`SystemWallClock`] implementation.
pub fn system_now_millis() -> u64 {
    SystemWallClock.now_millis()
}

/// Convenience free function, seconds variant of [`system_now_millis`].
pub fn system_now_secs() -> u64 {
    SystemWallClock.now_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_seconds_floor_milliseconds() {
        let clock = ManualWallClock::new(Timestamp(1_500));
        assert_eq!(clock.now_millis(), 1_500);
        assert_eq!(clock.now_secs(), 1);
    }

    #[test]
    fn manual_wall_clock_moves_forward_and_backward() {
        let clock = ManualWallClock::new(Timestamp(0));
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now(), Timestamp(2_000));
        clock.set(Timestamp(42));
        assert_eq!(clock.now(), Timestamp(42));
    }

    #[test]
    fn wall_clock_systemtime_conversion_preserves_milliseconds() {
        let clock = ManualWallClock::new(Timestamp(1_234));
        let expected = SystemTime::UNIX_EPOCH + Duration::from_millis(1_234);
        assert_eq!(clock.now_systemtime(), expected);
    }

    #[test]
    fn system_wall_clock_now_millis_is_close_to_wall_clock() {
        let before = SystemWallClock.now_millis();
        let observed = system_now_millis();
        let after = SystemWallClock.now_millis();
        assert!(before <= observed && observed <= after);
    }

    #[test]
    fn manual_monotonic_clock_advances_without_wall_clock_movement() {
        let wall = ManualWallClock::new(Timestamp(10_000));
        let monotonic = ManualMonotonicClock::new();
        let before = monotonic.now();

        let after = monotonic.advance(Duration::from_secs(3));

        assert_eq!(after.duration_since(before), Duration::from_secs(3));
        assert_eq!(wall.now(), Timestamp(10_000));
    }

    #[test]
    fn manual_wall_clock_moves_without_monotonic_clock_movement() {
        let wall = ManualWallClock::new(Timestamp(10_000));
        let monotonic = ManualMonotonicClock::new();
        let before = monotonic.now();

        wall.set(Timestamp(500));

        assert_eq!(wall.now(), Timestamp(500));
        assert_eq!(monotonic.now(), before);
    }
}
