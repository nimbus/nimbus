use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

pub async fn wait_for_condition<F, Fut>(
    description: &str,
    timeout: Duration,
    poll_interval: Duration,
    condition: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    wait_for_value(description, timeout, poll_interval, condition, |ready| {
        *ready
    })
    .await;
}

pub async fn wait_for_value<T, F, Fut, P>(
    description: &str,
    timeout: Duration,
    poll_interval: Duration,
    mut load: F,
    mut predicate: P,
) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
    P: FnMut(&T) -> bool,
{
    let started_at = Instant::now();
    loop {
        let value = load().await;
        if predicate(&value) {
            return value;
        }
        assert!(
            started_at.elapsed() < timeout,
            "timed out waiting for {description}"
        );
        if poll_interval.is_zero() {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(poll_interval).await;
        }
    }
}
