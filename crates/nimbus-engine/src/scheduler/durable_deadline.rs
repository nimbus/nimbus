use std::time::Duration;

use nimbus_core::Timestamp;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DurableDeadlineWake {
    ImmediateDue,
    Timer,
    Notified,
    Shutdown,
}

/// Waits for a durable wall-clock deadline without assuming wall time advances
/// at the same rate as Tokio's monotonic timer.
///
/// The notification future is armed before the wall observation so work that
/// moves the earliest deadline cannot be lost between inspection and sleep.
pub(super) async fn wait(
    engine: &Engine,
    durable_deadline: Option<Timestamp>,
    retry_deadline: Option<Instant>,
    max_resample_interval: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> DurableDeadlineWake {
    let notified = engine.scheduler_notifier().notified();
    tokio::pin!(notified);

    let delay = next_wait_duration(
        engine.now(),
        durable_deadline,
        Instant::now(),
        retry_deadline,
        max_resample_interval,
    );
    match delay {
        Some(delay) if delay.is_zero() => DurableDeadlineWake::ImmediateDue,
        Some(delay) => {
            let sleep = tokio::time::sleep(delay);
            tokio::pin!(sleep);
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        DurableDeadlineWake::Shutdown
                    } else {
                        DurableDeadlineWake::Timer
                    }
                }
                _ = &mut notified => DurableDeadlineWake::Notified,
                _ = &mut sleep => DurableDeadlineWake::Timer,
            }
        }
        None => {
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        DurableDeadlineWake::Shutdown
                    } else {
                        DurableDeadlineWake::Timer
                    }
                }
                _ = &mut notified => DurableDeadlineWake::Notified,
            }
        }
    }
}

fn next_wait_duration(
    wall_now: Timestamp,
    durable_deadline: Option<Timestamp>,
    monotonic_now: Instant,
    retry_deadline: Option<Instant>,
    max_resample_interval: Duration,
) -> Option<Duration> {
    let durable_delay = durable_deadline.map(|deadline| {
        if deadline <= wall_now {
            Duration::ZERO
        } else {
            bounded_wait_duration(
                deadline.saturating_duration_since(wall_now),
                max_resample_interval,
            )
        }
    });
    let retry_delay =
        retry_deadline.map(|deadline| deadline.saturating_duration_since(monotonic_now));
    match (durable_delay, retry_delay) {
        (Some(durable), Some(retry)) => Some(durable.min(retry)),
        (Some(durable), None) => Some(durable),
        (None, Some(retry)) => Some(retry),
        (None, None) => None,
    }
}

fn bounded_wait_duration(wall_delta: Duration, max_resample_interval: Duration) -> Duration {
    wall_delta.min(max_resample_interval.max(Duration::from_millis(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_clock_resampling_does_not_busy_loop() {
        assert_eq!(
            bounded_wait_duration(Duration::from_secs(60), Duration::ZERO),
            Duration::from_millis(1)
        );
        assert_eq!(
            bounded_wait_duration(Duration::from_millis(250), Duration::from_secs(1)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn retry_deadline_is_independent_of_durable_wall_time() {
        let monotonic_now = Instant::now();
        assert_eq!(
            next_wait_duration(
                Timestamp(1_000),
                None,
                monotonic_now,
                Some(monotonic_now + Duration::from_millis(250)),
                Duration::from_secs(1),
            ),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            next_wait_duration(
                Timestamp(1_000),
                Some(Timestamp(1_100)),
                monotonic_now,
                Some(monotonic_now + Duration::from_millis(250)),
                Duration::from_secs(1),
            ),
            Some(Duration::from_millis(100))
        );
    }
}
