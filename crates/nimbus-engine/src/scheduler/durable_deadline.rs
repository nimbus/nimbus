use std::time::Duration;

use nimbus_core::Timestamp;
use tokio::sync::watch;

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
    deadline: Timestamp,
    max_resample_interval: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> DurableDeadlineWake {
    let notified = engine.scheduler_notifier().notified();
    tokio::pin!(notified);

    let now = engine.now();
    if deadline <= now {
        return DurableDeadlineWake::ImmediateDue;
    }

    let wall_delta = deadline.saturating_duration_since(now);
    let delay = bounded_wait_duration(wall_delta, max_resample_interval);

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
}
