//! Shared fixed-interval condition polling for sandbox lifecycle waits.
//!
//! Several backends wait for a filesystem or command-reported condition to
//! become true by sleeping between checks (e.g. "has the runtime reached
//! `created`", "has the exit-status file appeared"). [`poll_until_deadline`]
//! is the one hand-rolled loop shape all of them share; callers keep their own
//! interval, deadline, and abort semantics by parameterizing the probe and the
//! (optional) deadline.

use std::thread;
use std::time::{Duration, Instant};

use crate::error::Result;

/// Poll `probe` every `interval` until it returns `Some(value)` or, when
/// `deadline` is set, the deadline elapses.
///
/// The deadline is checked *before* each probe (matching the original
/// per-backend loops), so a deadline that has already passed makes this
/// return `Ok(None)` without probing. A `deadline` of `None` polls forever —
/// it returns only once `probe` reports a value.
pub(crate) fn poll_until_deadline<T>(
    deadline: Option<Instant>,
    interval: Duration,
    mut probe: impl FnMut() -> Result<Option<T>>,
) -> Result<Option<T>> {
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(None);
        }
        if let Some(value) = probe()? {
            return Ok(Some(value));
        }
        thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_probed_value_once_it_appears() {
        let mut remaining_misses = 2;
        let result = poll_until_deadline(None, Duration::from_millis(1), || {
            if remaining_misses > 0 {
                remaining_misses -= 1;
                Ok(None)
            } else {
                Ok(Some("ready"))
            }
        });

        assert_eq!(result.unwrap(), Some("ready"));
    }

    #[test]
    fn expired_deadline_short_circuits_without_probing() {
        let mut probe_calls = 0;
        let expired = Instant::now() - Duration::from_secs(1);

        let result = poll_until_deadline(Some(expired), Duration::from_millis(1), || {
            probe_calls += 1;
            Ok(Some(()))
        });

        assert_eq!(result.unwrap(), None);
        assert_eq!(
            probe_calls, 0,
            "an already-expired deadline must skip the probe entirely"
        );
    }

    #[test]
    fn deadline_elapsing_mid_poll_returns_none() {
        let deadline = Instant::now() + Duration::from_millis(30);

        let result: Result<Option<()>> =
            poll_until_deadline(Some(deadline), Duration::from_millis(10), || Ok(None));

        assert_eq!(result.unwrap(), None);
    }
}
